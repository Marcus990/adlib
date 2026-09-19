//! Track C: "should the visual change?" (design doc §7.1). Jev only decides whether/how to
//! change; it never picks images. Transport: OpenRouter's decisions endpoint
//! (`POST https://openrouter.ai/api/alpha/decisions`, model `typesafe/jev-latest`), which takes the
//! same `{model, state, questions}` body and returns the same `answers` shape as TypeSafe's
//! native `/v1/systemone`.
//!
//! Fallbacks behind the same `ChangeDecision` output:
//! 1. Jev fails but a key exists → ask the chat query model for the action (doc §9 fallback).
//! 2. No key at all (offline dev/replay) → a transparent vocabulary heuristic.

use ls_contracts::{Action, ChangeDecision, Displayed};
use serde::Deserialize;
use std::collections::HashMap;
use std::time::Duration;

const DECISIONS_URL: &str = "https://openrouter.ai/api/alpha/decisions";
const CHAT_URL: &str = "https://openrouter.ai/api/v1/chat/completions";
pub const DEFAULT_JEV_MODEL: &str = "typesafe/jev-latest";

const INSTRUCTIONS: &str = "A presenter is speaking live and one picture is on screen behind them. \
`displayed` is the picture now on screen and `displayed.trigger_text` is what they said when it went up. \
`prev` is their previous phrase and `curr` is what they are saying right now. \
Should the picture change because of `curr`?";

pub fn criteria() -> serde_json::Value {
    serde_json::json!({
        "no_change": "They are still talking about what the picture shows, elaborating, or saying filler with no new picturable subject",
        "new_render": "They moved to a new topic or named a new concrete thing the audience would want to see",
        "update": "They are refining the same subject on screen (a different colour, angle, version or variant of it)",
        "clear": "They moved somewhere no picture should follow, such as a pause, a joke aside or wrapping up"
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Jev,
    LlmFallback,
    Heuristic,
}

#[derive(Clone)]
pub struct Decider {
    http: reqwest::Client,
    api_key: Option<String>,
    jev_model: String,
    chat_model: String,
    timeout: Duration,
    vocab: Vec<String>,
}

impl Decider {
    pub fn new(http: reqwest::Client, api_key: Option<String>, jev_model: Option<String>, chat_model: String, vocab: Vec<String>) -> Self {
        Self {
            http,
            api_key: api_key.filter(|k| !k.trim().is_empty()),
            jev_model: jev_model.unwrap_or_else(|| DEFAULT_JEV_MODEL.into()),
            chat_model,
            timeout: Duration::from_millis(900),
            vocab,
        }
    }

    pub fn has_remote(&self) -> bool {
        self.api_key.is_some()
    }

    pub fn build_request(&self, prev: &str, curr: &str, d: &Displayed, now_ms: u64) -> serde_json::Value {
        serde_json::json!({
            "model": self.jev_model,
            "state": state(prev, curr, d, now_ms),
            "questions": {
                "action": {"type": "choice", "instructions": INSTRUCTIONS, "criteria": criteria()}
            }
        })
    }

    pub async fn warm_up(&self) {
        if self.api_key.is_some() {
            let blank = Displayed { image_id: None, caption: None, trigger_text: String::new(), shown_at_ms: 0 };
            let _ = tokio::time::timeout(Duration::from_secs(5), self.jev("", "hello", &blank, 0)).await;
        }
    }

    /// Never fails: on any error it degrades to the next fallback. Returns the source for logging.
    pub async fn decide(&self, chunk_id: u64, seq: u64, prev: &str, curr: &str, d: &Displayed, now_ms: u64) -> (ChangeDecision, Source) {
        if self.api_key.is_some() {
            match tokio::time::timeout(self.timeout, self.jev(prev, curr, d, now_ms)).await {
                Ok(Ok((action, p))) => return (ChangeDecision { chunk_id, seq, action, p }, Source::Jev),
                Ok(Err(e)) => eprintln!("decide: jev error: {e:#}"),
                Err(_) => eprintln!("decide: jev timed out after {:?}", self.timeout),
            }
            match tokio::time::timeout(self.timeout, self.llm(prev, curr, d)).await {
                Ok(Ok(action)) => return (ChangeDecision { chunk_id, seq, action, p: 0.7 }, Source::LlmFallback),
                Ok(Err(e)) => eprintln!("decide: llm fallback error: {e:#}"),
                Err(_) => eprintln!("decide: llm fallback timed out"),
            }
        }
        let (action, p) = heuristic(curr, d, &self.vocab);
        (ChangeDecision { chunk_id, seq, action, p }, Source::Heuristic)
    }

    async fn jev(&self, prev: &str, curr: &str, d: &Displayed, now_ms: u64) -> anyhow::Result<(Action, f32)> {
        let body = self.build_request(prev, curr, d, now_ms);
        let resp = self.http.post(DECISIONS_URL).bearer_auth(self.api_key.as_deref().unwrap_or_default()).json(&body).send().await?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("HTTP {status}: {}", &text[..text.len().min(300)]);
        }
        parse_jev(&text)
    }

    async fn llm(&self, prev: &str, curr: &str, d: &Displayed) -> anyhow::Result<Action> {
        let prompt = format!(
            "{INSTRUCTIONS}\nOptions: {}\nState: {}\nReply with JSON only: {{\"action\": \"<option>\"}}",
            criteria(),
            state(prev, curr, d, d.shown_at_ms)
        );
        let body = serde_json::json!({
            "model": self.chat_model,
            "messages": [{"role": "user", "content": prompt}],
            "response_format": {"type": "json_object"},
            "temperature": 0, "max_tokens": 20,
            "provider": {"sort": "latency"}
        });
        let resp = self.http.post(CHAT_URL).bearer_auth(self.api_key.as_deref().unwrap_or_default()).json(&body).send().await?;
        let v: serde_json::Value = resp.json().await?;
        let content = v["choices"][0]["message"]["content"].as_str().unwrap_or_default();
        let start = content.find('{').unwrap_or(0);
        let end = content.rfind('}').map(|i| i + 1).unwrap_or(content.len());
        let a: serde_json::Value = serde_json::from_str(&content[start..end])?;
        parse_action(a["action"].as_str().unwrap_or_default())
    }
}

fn state(prev: &str, curr: &str, d: &Displayed, now_ms: u64) -> serde_json::Value {
    serde_json::json!({
        "prev": prev,
        "curr": curr,
        "displayed": {
            "caption": d.caption.clone().unwrap_or_else(|| "nothing (blank screen)".into()),
            "trigger_text": d.trigger_text,
            "seconds_on_screen": now_ms.saturating_sub(d.shown_at_ms) / 1000
        }
    })
}

pub fn parse_action(s: &str) -> anyhow::Result<Action> {
    Ok(match s {
        "no_change" => Action::NoChange,
        "new_render" => Action::NewRender,
        "update" => Action::Update,
        "clear" => Action::Clear,
        other => anyhow::bail!("unknown action {other:?}"),
    })
}

#[derive(Deserialize)]
struct JevResponse {
    answers: HashMap<String, JevAnswer>,
}
#[derive(Deserialize)]
struct JevAnswer {
    choice: Option<String>,
    #[serde(default)]
    probabilities: HashMap<String, f32>,
    confidence: Option<f32>,
}

/// Parse `{answers: {action: {choice, probabilities, confidence}}}`. P = probability of the chosen
/// option (falls back to confidence).
pub fn parse_jev(body: &str) -> anyhow::Result<(Action, f32)> {
    let r: JevResponse = serde_json::from_str(body)?;
    let a = r.answers.get("action").ok_or_else(|| anyhow::anyhow!("no `action` answer in {body}"))?;
    let choice = a.choice.clone().ok_or_else(|| anyhow::anyhow!("no choice in {body}"))?;
    let p = a.probabilities.get(&choice).copied().or(a.confidence).unwrap_or(0.0);
    Ok((parse_action(&choice)?, p))
}

/// Offline-only heuristic (no API key): change when the newest speech names a library subject
/// that is not what's on screen. Deliberately simple and transparent.
pub fn heuristic(curr: &str, d: &Displayed, vocab: &[String]) -> (Action, f32) {
    let lower = curr.to_lowercase();
    let on_screen = d.caption.clone().unwrap_or_default().to_lowercase();
    for cap in vocab {
        let subject = cap.split('(').next().unwrap_or(cap).trim().to_lowercase();
        if subject.len() < 3 || on_screen.starts_with(&subject) {
            continue;
        }
        let hit = lower
            .split(|c: char| !c.is_alphanumeric())
            .any(|w| w == subject || w.trim_end_matches('s') == subject || subject.split(' ').any(|s| s.len() > 3 && s == w));
        if hit {
            return (Action::NewRender, 0.8);
        }
    }
    (Action::NoChange, 0.9)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn disp(caption: Option<&str>) -> Displayed {
        Displayed { image_id: None, caption: caption.map(String::from), trigger_text: "t".into(), shown_at_ms: 1000 }
    }

    #[test]
    fn request_has_the_documented_shape() {
        let dcd = Decider::new(reqwest::Client::new(), Some("k".into()), None, "m".into(), vec![]);
        let v = dcd.build_request("prev", "curr", &disp(Some("eagle")), 6000);
        assert_eq!(v["model"], "typesafe/jev-latest");
        assert_eq!(v["state"]["curr"], "curr");
        assert_eq!(v["state"]["displayed"]["caption"], "eagle");
        assert_eq!(v["state"]["displayed"]["seconds_on_screen"], 5);
        assert_eq!(v["questions"]["action"]["type"], "choice");
        let crit = v["questions"]["action"]["criteria"].as_object().unwrap();
        assert_eq!(crit.len(), 4);
        assert!(crit.contains_key("new_render") && crit.contains_key("clear"));
    }

    #[test]
    fn parses_documented_response() {
        let body = r#"{"id":"gen-1","model":"jev-1.13.0","provider":"TypeSafe","answers":{"action":{"type":"choice","choice":"new_render","confidence":0.8,"probabilities":{"no_change":0.1,"new_render":0.85,"update":0.03,"clear":0.02}}},"usage":{"input_tokens":312,"output_tokens":48,"cost":0.00001}}"#;
        assert_eq!(parse_jev(body).unwrap(), (Action::NewRender, 0.85));
    }

    #[test]
    fn parse_falls_back_to_confidence() {
        let body = r#"{"answers":{"action":{"type":"choice","choice":"no_change","confidence":0.7}}}"#;
        assert_eq!(parse_jev(body).unwrap(), (Action::NoChange, 0.7));
    }

    #[test]
    fn heuristic_changes_on_new_library_subject_only() {
        let vocab = vec!["eagle (animals)".to_string(), "guitar (instruments)".to_string()];
        assert_eq!(heuristic("and then an eagle flew over", &disp(None), &vocab).0, Action::NewRender);
        assert_eq!(heuristic("the eagle again", &disp(Some("eagle (animals)")), &vocab).0, Action::NoChange);
        assert_eq!(heuristic("nothing relevant here", &disp(None), &vocab).0, Action::NoChange);
        assert_eq!(heuristic("I play guitars", &disp(Some("eagle (animals)")), &vocab).0, Action::NewRender);
    }

    #[tokio::test]
    async fn no_key_is_heuristic() {
        let dcd = Decider::new(reqwest::Client::new(), None, None, "m".into(), vec!["zebra (animals)".into()]);
        let (d, src) = dcd.decide(3, 9, "", "look at that zebra", &disp(None), 0).await;
        assert_eq!(src, Source::Heuristic);
        assert_eq!((d.chunk_id, d.seq, d.action), (3, 9, Action::NewRender));
    }
}
