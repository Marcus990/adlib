//! Canvas agent (CANVAS.md): a chat model with tools that refines the evolving board after the fast
//! path has placed an image. Model-agnostic over OpenRouter (CANVAS_MODEL, default Claude Haiku 4.5;
//! bake-off vs Gemini Flash-Lite once a key exists). No key / error / timeout → offline cue rules.

use ls_canvas::{has_section_cue, rule_ops, AnnotationKind, ChartKind, DiagramLayout, EdgeSpec, ElementKind, Layout, NodeSpec, Op, Point, Scene};
use serde_json::{json, Value};
use std::time::Duration;

pub const DEFAULT_MODEL: &str = "anthropic/claude-haiku-4.5";

fn base() -> String {
    std::env::var("OPENROUTER_BASE_URL").unwrap_or_else(|_| "https://openrouter.ai".into())
}

const SYSTEM: &str = "You are the live designer for a talk. Act only on `newest_speech`; `previous_speech` is \
context that was already handled — never act on it again. The screen behind the presenter is a board of up to 4 \
tiles: photos (added automatically when the presenter talks about something picturable), diagrams and charts \
(added by YOU), plus up to 3 annotations. Build visuals that SUPPLEMENT what is being said, as it is said.\n\
DIAGRAMS — when the speech describes structure: steps or a process (draw_diagram flow), cause and effect \
(flow with edge labels), something that repeats (cycle), a central idea and its parts (hub), or dated events \
(timeline, year in `note`). 2-8 nodes, labels of 1-4 words taken from the speech, an optional single emoji \
`icon` per node. When the presenter keeps describing the SAME structure, call extend_diagram with only the \
new nodes instead of drawing a new one. If a node on the board was misheard or is now clearer (speech arrives \
in fragments and early words can be wrong), call draw_diagram again with the full corrected node list — it \
replaces the old one in place. Don't add a node that repeats one already there.\n\
CHARTS — only from numbers the presenter actually says (never invent or estimate data): values over time or \
across groups (bar; line for a trend over 3+ times), shares of a whole (pie), one headline number or a \
before → after (stat). Use plain numbers in `value` (\"fifteen thousand\" → 15000, \"60 percent\" → 60 with \
unit \"%\"). Every value must be one the presenter said: never add an \"Other\", \"Rest\" or \"Not X\" \
remainder — a pie may sum to less than 100. When they add a number or the transcript firms up (early words \
can be misheard), call update_chart with the full corrected list of values.\n\
LAYOUT — compare two things (arrange compare), zoom in on one (focus + arrange hero), everything together \
(arrange grid), draw attention (annotate highlight), link two tiles (annotate arrow), clear the board when \
they move to a new section.\n\
If the newest sentence is unfinished, or there is nothing to structure or count, call NO tool — that is the \
right answer most of the time. Never invent element ids; use the ids listed on the board.";

pub fn tools() -> Value {
    let id = json!({"type": "string", "description": "element id from the board, e.g. e3"});
    let node = json!({"type": "object", "properties": {
        "label": {"type": "string", "description": "1–4 words"},
        "icon": {"type": "string", "description": "optional single emoji"},
        "note": {"type": "string", "description": "optional: a year or ≤ 3-word detail"}}, "required": ["label"]});
    let edge = json!({"type": "object", "properties": {
        "from": {"type": "string", "description": "node label"}, "to": {"type": "string", "description": "node label"},
        "label": {"type": "string", "description": "optional ≤ 3 words, e.g. 'causes'"}}, "required": ["from", "to"]});
    let point = json!({"type": "object", "properties": {"label": {"type": "string"}, "value": {"type": "number"}}, "required": ["label", "value"]});
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
            "parameters": {"type": "object", "properties": {}}}},
        {"type": "function", "function": {"name": "draw_diagram", "description": "Add a diagram tile that shows structure the presenter is describing.",
            "parameters": {"type": "object", "properties": {
                "layout": {"type": "string", "enum": ["flow", "cycle", "hub", "timeline"],
                    "description": "flow: steps / cause→effect left to right; cycle: steps that repeat; hub: first node is the centre, others are its parts; timeline: dated events (put the date in note)"},
                "title": {"type": "string", "description": "≤ 6 words"},
                "nodes": {"type": "array", "items": node.clone(), "description": "2–8 nodes in order"},
                "edges": {"type": "array", "items": edge.clone(), "description": "optional; omit for a simple chain (flow/cycle/timeline) or spokes (hub)"}},
                "required": ["layout", "nodes"]}}},
        {"type": "function", "function": {"name": "extend_diagram", "description": "Add nodes (and optional edges) to a diagram already on the board.",
            "parameters": {"type": "object", "properties": {"id": id.clone(),
                "nodes": {"type": "array", "items": node},
                "edges": {"type": "array", "items": edge}},
                "required": ["id", "nodes"]}}},
        {"type": "function", "function": {"name": "draw_chart", "description": "Add a chart tile built from numbers the presenter said.",
            "parameters": {"type": "object", "properties": {
                "kind": {"type": "string", "enum": ["bar", "line", "pie", "stat"],
                    "description": "bar: compare values; line: trend over 3+ times; pie: shares of a whole; stat: one headline number, or before→after with 2 points"},
                "title": {"type": "string", "description": "≤ 6 words"},
                "unit": {"type": "string", "description": "e.g. %, $, users, km"},
                "points": {"type": "array", "items": point.clone(), "description": "in the order spoken (chronological for time)"}},
                "required": ["kind", "points"]}}},
        {"type": "function", "function": {"name": "update_chart", "description": "Replace a chart's data with the FULL corrected list of values (include the ones already there that are still right).",
            "parameters": {"type": "object", "properties": {"id": id,
                "kind": {"type": "string", "enum": ["bar", "line", "pie", "stat"]},
                "title": {"type": "string", "description": "optional new title if the chart's meaning grew"},
                "points": {"type": "array", "items": point}},
                "required": ["id", "points"]}}}
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
            // Graphics are 100–300 output tokens (Haiku 4.5 ≈ 1.5–3 s); layout-only calls return sooner.
            timeout: Duration::from_millis(5000),
        }
    }

    pub fn has_remote(&self) -> bool {
        self.api_key.is_some()
    }

    /// Ops for the current board given the newest speech. Never fails.
    pub async fn propose(&self, scene: &Scene, prev: &str, curr: &str) -> (Vec<Op>, Source) {
        if self.api_key.is_some() {
            match tokio::time::timeout(self.timeout, self.remote(scene, prev, curr)).await {
                Ok(Ok(ops)) => return (ground(ops, scene, prev, curr), Source::Model),
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
            .map(|e| match e.kind {
                ElementKind::Image => json!({"id": e.id, "photo": e.caption, "focus": e.focus}),
                ElementKind::Diagram => {
                    let d = e.diagram.as_ref();
                    json!({"id": e.id, "diagram": d.map(|d| d.layout), "title": d.and_then(|d| d.title.clone()), "focus": e.focus,
                        "nodes": d.map(|d| d.nodes.iter().map(|n| n.label.clone()).collect::<Vec<_>>()).unwrap_or_default()})
                }
                ElementKind::Chart => {
                    let c = e.chart.as_ref();
                    json!({"id": e.id, "chart": c.map(|c| c.kind), "title": c.and_then(|c| c.title.clone()), "unit": c.and_then(|c| c.unit.clone()),
                        "focus": e.focus, "points": c.map(|c| c.points.iter().map(|p| json!([p.label, p.value])).collect::<Vec<_>>()).unwrap_or_default()})
                }
            })
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
            "max_tokens": 700,
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

/// Code-level guards on model output (prompt rules alone were not reliable, 09-19 probes):
/// - `clear_board` only when the newest words close a section ("let's move on") — the model also
///   cleared on a *previous* sentence's "moving on", wiping photos that had just appeared;
/// - chart values must be numbers the presenter said (or already on the board): drops invented
///   remainders like "Not stoned: 40" from "60% of them are stoned".
pub fn ground(ops: Vec<Op>, scene: &Scene, prev: &str, curr: &str) -> Vec<Op> {
    let mut said = spoken_numbers(&format!("{prev} {curr}"));
    for e in &scene.elements {
        if let Some(c) = &e.chart {
            said.extend(c.points.iter().map(|p| p.value));
        }
    }
    let grounded = |v: f64| said.iter().any(|s| (s - v).abs() <= 0.005 * s.abs().max(v.abs()) + 1e-9);
    ops.into_iter()
        .filter_map(|op| match op {
            Op::ClearBoard if !has_section_cue(curr) => None,
            Op::DrawChart { kind, title, unit, points } => {
                let points: Vec<Point> = points.into_iter().filter(|p| grounded(p.value)).collect();
                (!points.is_empty()).then_some(Op::DrawChart { kind, title, unit, points })
            }
            Op::UpdateChart { id, kind, title, points } => {
                let points: Vec<Point> = points.into_iter().filter(|p| grounded(p.value)).collect();
                (!points.is_empty() || title.is_some()).then_some(Op::UpdateChart { id, kind, title, points })
            }
            other => Some(other),
        })
        .collect()
}

/// Numbers in speech: digits ("15,000", "1.2", "60%", "$5") with an optional scale word ("1.2 million"),
/// and number words ("forty thousand", "two hundred and fifty"). Both the bare and the scaled value
/// are returned so either form grounds a chart value.
pub fn spoken_numbers(text: &str) -> Vec<f64> {
    const UNITS: [&str; 20] = ["zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven",
        "twelve", "thirteen", "fourteen", "fifteen", "sixteen", "seventeen", "eighteen", "nineteen"];
    const TENS: [&str; 8] = ["twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety"];
    let scale = |w: &str| match w {
        "hundred" => Some(100.0),
        "thousand" | "k" => Some(1e3),
        "million" | "m" => Some(1e6),
        "billion" | "b" => Some(1e9),
        _ => None,
    };
    let lower = text.to_lowercase().replace('-', " ");
    let words: Vec<String> = lower
        .split(|c: char| c.is_whitespace() || matches!(c, '(' | ')' | '"' | ';' | ':' | '!' | '?'))
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '.').trim_end_matches('.').to_string())
        .filter(|w| !w.is_empty())
        .collect();
    let mut out = vec![];
    let (mut total, mut cur, mut in_words) = (0.0f64, 0.0f64, false);
    let flush = |total: &mut f64, cur: &mut f64, in_words: &mut bool, out: &mut Vec<f64>| {
        if *in_words {
            out.push(*total + *cur);
        }
        *total = 0.0;
        *cur = 0.0;
        *in_words = false;
    };
    for (i, w) in words.iter().enumerate() {
        let digits: String = w.chars().filter(|c| c.is_ascii_digit() || *c == '.').collect();
        if w.chars().next().is_some_and(|c| c.is_ascii_digit() || c == '$') && !digits.is_empty() {
            flush(&mut total, &mut cur, &mut in_words, &mut out);
            if let Ok(v) = digits.trim_end_matches('.').parse::<f64>() {
                out.push(v);
                let suffix = w.trim_end_matches('%').chars().last().map(|c| c.to_string()).unwrap_or_default();
                if let Some(m) = scale(&suffix).or_else(|| words.get(i + 1).and_then(|n| scale(n))) {
                    out.push(v * m);
                }
            }
        } else if let Some(u) = UNITS.iter().position(|x| x == w) {
            cur += u as f64;
            in_words = true;
        } else if let Some(t) = TENS.iter().position(|x| x == w) {
            cur += (t as f64 + 2.0) * 10.0;
            in_words = true;
        } else if let Some(m) = scale(w).filter(|_| w.len() > 1) {
            if in_words || matches!(i.checked_sub(1).and_then(|j| words.get(j)).map(|s| s.as_str()), Some("a" | "an")) {
                let base = if cur == 0.0 { 1.0 } else { cur };
                if m >= 1e3 {
                    total += base * m;
                    cur = 0.0;
                } else {
                    cur = base * m;
                }
                in_words = true;
            }
        } else if w == "and" && in_words {
            // "two hundred and fifty"
        } else {
            flush(&mut total, &mut cur, &mut in_words, &mut out);
        }
    }
    flush(&mut total, &mut cur, &mut in_words, &mut out);
    out
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
                "draw_diagram" => Op::DrawDiagram {
                    layout: match s("layout")?.as_str() {
                        "flow" => DiagramLayout::Flow,
                        "cycle" => DiagramLayout::Cycle,
                        "hub" => DiagramLayout::Hub,
                        "timeline" => DiagramLayout::Timeline,
                        _ => return None,
                    },
                    title: s("title").filter(|t| !t.trim().is_empty()),
                    nodes: nodes(&args["nodes"]),
                    edges: edges(&args["edges"]),
                },
                "extend_diagram" => Op::ExtendDiagram { id: s("id")?, nodes: nodes(&args["nodes"]), edges: edges(&args["edges"]) },
                "draw_chart" => Op::DrawChart {
                    kind: chart_kind(&s("kind")?)?,
                    title: s("title").filter(|t| !t.trim().is_empty()),
                    unit: s("unit").filter(|t| !t.trim().is_empty()),
                    points: points(&args["points"]),
                },
                "update_chart" => Op::UpdateChart { id: s("id")?, kind: s("kind").and_then(|k| chart_kind(&k)), title: s("title").filter(|t| !t.trim().is_empty()), points: points(&args["points"]) },
                _ => return None,
            })
        })
        .collect()
}

fn chart_kind(k: &str) -> Option<ChartKind> {
    match k {
        "bar" => Some(ChartKind::Bar),
        "line" => Some(ChartKind::Line),
        "pie" => Some(ChartKind::Pie),
        "stat" => Some(ChartKind::Stat),
        _ => None,
    }
}

// Lenient: models sometimes send nodes as bare strings, numbers as strings ("15,000"), edges as pairs.
fn nodes(v: &Value) -> Vec<NodeSpec> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|n| match n {
                    Value::String(l) => Some(NodeSpec { label: l.clone(), icon: None, note: None }),
                    Value::Object(_) => Some(NodeSpec {
                        label: n["label"].as_str()?.to_string(),
                        icon: n["icon"].as_str().map(String::from),
                        note: n["note"].as_str().map(String::from).or_else(|| n["note"].as_i64().map(|y| y.to_string())),
                    }),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

fn edges(v: &Value) -> Vec<EdgeSpec> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|e| match e {
                    Value::Array(p) if p.len() >= 2 => Some(EdgeSpec { from: p[0].as_str()?.into(), to: p[1].as_str()?.into(), label: p.get(2).and_then(|l| l.as_str()).map(String::from) }),
                    Value::Object(_) => Some(EdgeSpec { from: e["from"].as_str()?.into(), to: e["to"].as_str()?.into(), label: e["label"].as_str().map(String::from) }),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

fn number(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_str().and_then(|s| s.replace([',', '%', '$'], "").trim().parse().ok()))
}

fn points(v: &Value) -> Vec<Point> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|p| match p {
                    Value::Array(x) if x.len() >= 2 => Some(Point { label: x[0].as_str().map(String::from).unwrap_or_else(|| x[0].to_string()), value: number(&x[1])? }),
                    Value::Object(_) => Some(Point {
                        label: p["label"].as_str().map(String::from).unwrap_or_else(|| p["label"].to_string()),
                        value: number(&p["value"])?,
                    }),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
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
        assert_eq!(v["tools"].as_array().unwrap().len(), 10);
        let user: Value = serde_json::from_str(v["messages"][1]["content"].as_str().unwrap()).unwrap();
        assert_eq!(user["board"][0]["id"], "e1");
        assert_eq!(user["newest_speech"], "notice the beak");
    }

    #[test]
    fn parses_graphic_tools_leniently() {
        let body = r#"{"choices":[{"message":{"tool_calls":[
            {"function":{"name":"draw_diagram","arguments":"{\"layout\":\"flow\",\"title\":\"How it works\",\"nodes\":[{\"label\":\"Speak\",\"icon\":\"🎤\"},\"Transcribe\"],\"edges\":[[\"Speak\",\"Transcribe\",\"then\"]]}"}},
            {"function":{"name":"draw_chart","arguments":"{\"kind\":\"bar\",\"unit\":\"users\",\"points\":[{\"label\":\"2024\",\"value\":2000},{\"label\":2025,\"value\":\"15,000\"},[\"2026\",40000]]}"}},
            {"function":{"name":"update_chart","arguments":"{\"id\":\"e2\",\"kind\":\"donut\",\"points\":[]}"}},
            {"function":{"name":"draw_chart","arguments":"{\"kind\":\"radar\",\"points\":[]}"}}]}}]}"#;
        let ops = parse_tool_calls(body);
        assert_eq!(ops.len(), 3, "{ops:?}");
        match &ops[0] {
            Op::DrawDiagram { layout, nodes, edges, .. } => {
                assert_eq!(*layout, DiagramLayout::Flow);
                assert_eq!(nodes[0].icon.as_deref(), Some("🎤"));
                assert_eq!(nodes[1].label, "Transcribe");
                assert_eq!(edges[0].label.as_deref(), Some("then"));
            }
            o => panic!("{o:?}"),
        }
        match &ops[1] {
            Op::DrawChart { points, .. } => assert_eq!(points.iter().map(|p| p.value).collect::<Vec<_>>(), vec![2000.0, 15000.0, 40000.0]),
            o => panic!("{o:?}"),
        }
        assert!(matches!(&ops[2], Op::UpdateChart { kind: None, .. }), "unknown kind → keep current");
    }

    #[test]
    fn spoken_numbers_cover_digits_words_and_scales() {
        let n = spoken_numbers("Last year about two thousand users; this year 15,000, next year forty thousand. 60% are students, $1.2 million raised, two hundred and fifty stores.");
        for v in [2000.0, 15000.0, 40000.0, 60.0, 1.2, 1_200_000.0, 250.0] {
            assert!(n.iter().any(|x| (x - v).abs() < 1e-6), "{v} missing from {n:?}");
        }
    }

    #[test]
    fn ground_drops_invented_values_and_unasked_clears() {
        let mut c = ls_canvas::Canvas::new();
        c.render("sunflower", "sunflower", "u", 1);
        let pie = |pts: &[(&str, f64)]| Op::DrawChart { kind: ChartKind::Pie, title: None, unit: Some("%".into()),
            points: pts.iter().map(|(l, v)| Point { label: l.to_string(), value: *v }).collect() };
        let ops = ground(vec![pie(&[("Stoned", 60.0), ("Not stoned", 40.0)]), Op::ClearBoard], c.scene(), "OK, moving on to a new section.", "About 60% of them are stoned.");
        assert_eq!(ops.len(), 1, "clear dropped: the section cue was in the previous sentence");
        match &ops[0] {
            Op::DrawChart { points, .. } => assert_eq!(points.len(), 1, "40 was never said"),
            o => panic!("{o:?}"),
        }
        assert_eq!(ground(vec![Op::ClearBoard], c.scene(), "", "Great. Let's move on.").len(), 1);
        assert!(ground(vec![pie(&[("Other", 40.0)])], c.scene(), "", "about sixty percent").is_empty());
    }

    /// `cargo test -p ls-agent dump_tools -- --ignored --nocapture` → the tool schema, for prompt probes.
    #[test]
    #[ignore]
    fn dump_tools() {
        println!("TOOLS_JSON {}", tools());
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
