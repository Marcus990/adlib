//! Canvas agent (CANVAS.md): a chat model with tools that refines the evolving board after the fast
//! path has placed an image. Model-agnostic over OpenRouter (CANVAS_MODEL, default Claude Haiku 4.5;
//! bake-off vs Gemini Flash-Lite once a key exists). No key / error / timeout → offline cue rules.

use ls_canvas::{rule_ops, AnnotationKind, Layout, Op, Scene};
use serde_json::{json, Value};
use std::time::Duration;

pub const DEFAULT_MODEL: &str = "anthropic/claude-haiku-4.5";

fn base() -> String {
    std::env::var("OPENROUTER_BASE_URL").unwrap_or_else(|_| "https://openrouter.ai".into())
}

const SYSTEM: &str = "You are the stage manager for a live talk. The screen is a board of up to 4 pictures \
(and up to 3 annotations) that illustrate what the presenter is saying; pictures are added automatically \
when the presenter points at something. Your job is only to REFINE the board when the presenter's newest \
words call for it: compare two things (arrange compare), zoom in on one (focus + arrange hero), show \
everything together (arrange grid), draw attention to a detail (annotate highlight), link two pictures \
(annotate arrow), or clear the board when they move to a new section. Most of the time the right answer is \
to call NO tool. Never invent element ids — use the ids listed. Keep labels under 5 words.";

pub fn tools() -> Value {
    let id = json!({"type": "string", "description": "element id from the board, e.g. e3"});
    json!([
        {"type": "function", "function": {"name": "focus", "description": "Make one picture the focus.",
            "parameters": {"type": "object", "properties": {"id": id}, "required": ["id"]}}},
        {"type": "function", "function": {"name": "remove", "description": "Take one picture off the board.",
            "parameters": {"type": "object", "properties": {"id": id}, "required": ["id"]}}},
        {"type": "function", "function": {"name": "arrange", "description": "Re-lay out the board.",
            "parameters": {"type": "object", "properties": {"layout": {"type": "string", "enum": ["auto", "hero", "compare", "grid"],
                "description": "hero: focus big + others small; compare: side by side; grid: all equal; auto: by count"}},
                "required": ["layout"]}}},
        {"type": "function", "function": {"name": "annotate", "description": "Draw attention: highlight/frame one picture, or an arrow between two.",
            "parameters": {"type": "object", "properties": {
                "kind": {"type": "string", "enum": ["highlight", "frame", "arrow"]},
                "targets": {"type": "array", "items": {"type": "string"}, "description": "element ids (2 for arrow)"},
                "label": {"type": "string", "description": "optional, ≤ 5 words"}},
                "required": ["kind", "targets"]}}},
        {"type": "function", "function": {"name": "clear_annotations", "description": "Remove all annotations.",
            "parameters": {"type": "object", "properties": {}}}},
        {"type": "function", "function": {"name": "clear_board", "description": "Clear the board (new section of the talk).",
            "parameters": {"type": "object", "properties": {}}}}
    ])
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Model,
    Rules,
}

#[derive(Clone)]
pub struct CanvasAgent {
    http: reqwest::Client,
    api_key: Option<String>,
    pub model: String,
    timeout: Duration,
}

impl CanvasAgent {
    pub fn new(http: reqwest::Client, api_key: Option<String>, model: Option<String>) -> Self {
        Self {
            http,
            api_key: api_key.filter(|k| !k.trim().is_empty()),
            model: model.unwrap_or_else(|| DEFAULT_MODEL.into()),
            timeout: Duration::from_millis(2500),
        }
    }

    pub fn has_remote(&self) -> bool {
        self.api_key.is_some()
    }

    /// Ops for the current board given the newest speech. Never fails.
    pub async fn propose(&self, scene: &Scene, prev: &str, curr: &str) -> (Vec<Op>, Source) {
        if self.api_key.is_some() {
            match tokio::time::timeout(self.timeout, self.remote(scene, prev, curr)).await {
                Ok(Ok(ops)) => return (ops, Source::Model),
                Ok(Err(e)) => eprintln!("canvas agent: {e:#}"),
                Err(_) => eprintln!("canvas agent: timed out after {:?}", self.timeout),
            }
        }
        (rule_ops(curr, scene), Source::Rules)
    }

    pub fn request_body(&self, scene: &Scene, prev: &str, curr: &str) -> Value {
        let board: Vec<Value> = scene
            .elements
            .iter()
            .map(|e| json!({"id": e.id, "picture": e.caption, "focus": e.focus}))
            .collect();
        let notes: Vec<Value> = scene
            .annotations
            .iter()
            .map(|a| json!({"kind": a.kind, "targets": a.targets, "label": a.label}))
            .collect();
        let user = json!({"board": board, "layout": scene.layout, "annotations": notes, "previous_speech": prev, "newest_speech": curr});
        json!({
            "model": self.model,
            "messages": [{"role": "system", "content": SYSTEM}, {"role": "user", "content": user.to_string()}],
            "tools": tools(),
            "tool_choice": "auto",
            "temperature": 0,
            "max_tokens": 200,
            "provider": {"sort": "latency"}
        })
    }

    async fn remote(&self, scene: &Scene, prev: &str, curr: &str) -> anyhow::Result<Vec<Op>> {
        let body = self.request_body(scene, prev, curr);
        let resp = self
            .http
            .post(format!("{}/api/v1/chat/completions", base()))
            .bearer_auth(self.api_key.as_deref().unwrap_or_default())
            .json(&body)
            .send()
            .await?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("HTTP {status}: {}", &text[..text.len().min(300)]);
        }
        Ok(parse_tool_calls(&text))
    }
}

/// OpenAI-style `choices[0].message.tool_calls[*].function.{name, arguments}` → ops. Unknown tools and
/// malformed arguments are skipped (the canvas validates ids again when applying).
pub fn parse_tool_calls(body: &str) -> Vec<Op> {
    let Ok(v) = serde_json::from_str::<Value>(body) else { return vec![] };
    let calls = v["choices"][0]["message"]["tool_calls"].as_array().cloned().unwrap_or_default();
    calls
        .iter()
        .filter_map(|c| {
            let name = c["function"]["name"].as_str()?;
            let args: Value = match &c["function"]["arguments"] {
                Value::String(s) => serde_json::from_str(s).ok()?,
                other => other.clone(),
            };
            let s = |k: &str| args[k].as_str().map(String::from);
            Some(match name {
                "focus" => Op::Focus { id: s("id")? },
                "remove" => Op::Remove { id: s("id")? },
                "arrange" => Op::Arrange {
                    layout: match s("layout")?.as_str() {
                        "hero" => Layout::Hero,
                        "compare" => Layout::Compare,
                        "grid" => Layout::Grid,
                        "auto" => Layout::Auto,
                        _ => return None,
                    },
                },
                "annotate" => Op::Annotate {
                    kind: match s("kind")?.as_str() {
                        "highlight" => AnnotationKind::Highlight,
                        "frame" => AnnotationKind::Frame,
                        "arrow" => AnnotationKind::Arrow,
                        _ => return None,
                    },
                    targets: args["targets"].as_array()?.iter().filter_map(|t| t.as_str().map(String::from)).collect(),
                    label: s("label").filter(|l| !l.trim().is_empty()),
                },
                "clear_annotations" => Op::ClearAnnotations,
                "clear_board" => Op::ClearBoard,
                _ => return None,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ls_canvas::Canvas;

    #[test]
    fn parses_tool_calls_and_skips_junk() {
        let body = r#"{"choices":[{"message":{"role":"assistant","content":null,"tool_calls":[
            {"id":"1","type":"function","function":{"name":"arrange","arguments":"{\"layout\":\"compare\"}"}},
            {"id":"2","type":"function","function":{"name":"annotate","arguments":"{\"kind\":\"highlight\",\"targets\":[\"e1\"],\"label\":\"the beak\"}"}},
            {"id":"3","type":"function","function":{"name":"teleport","arguments":"{}"}},
            {"id":"4","type":"function","function":{"name":"arrange","arguments":"{\"layout\":\"spiral\"}"}},
            {"id":"5","type":"function","function":{"name":"focus","arguments":"not json"}}]}}]}"#;
        assert_eq!(
            parse_tool_calls(body),
            vec![
                Op::Arrange { layout: Layout::Compare },
                Op::Annotate { kind: AnnotationKind::Highlight, targets: vec!["e1".into()], label: Some("the beak".into()) },
            ]
        );
        assert!(parse_tool_calls(r#"{"choices":[{"message":{"content":"nothing to do"}}]}"#).is_empty());
    }

    #[test]
    fn request_lists_board_and_tools() {
        let mut c = Canvas::new();
        c.render("eagle", "eagle (animals)", "img://localhost/eagle", 1);
        let a = CanvasAgent::new(reqwest::Client::new(), Some("k".into()), None);
        let v = a.request_body(c.scene(), "p", "notice the beak");
        assert_eq!(v["model"], DEFAULT_MODEL);
        assert_eq!(v["tools"].as_array().unwrap().len(), 6);
        let user: Value = serde_json::from_str(v["messages"][1]["content"].as_str().unwrap()).unwrap();
        assert_eq!(user["board"][0]["id"], "e1");
        assert_eq!(user["newest_speech"], "notice the beak");
    }

    #[tokio::test]
    async fn no_key_uses_rules() {
        let mut c = Canvas::new();
        c.render("eagle", "eagle", "u", 1);
        c.render("owl", "owl", "u", 2);
        let a = CanvasAgent::new(reqwest::Client::new(), None, None);
        let (ops, src) = a.propose(c.scene(), "", "let's compare them side by side").await;
        assert_eq!(src, Source::Rules);
        assert_eq!(ops, vec![Op::Arrange { layout: Layout::Compare }]);
    }
}
