//! Track C: "should the visual change?" (design doc §7.1). Jev only decides whether/how to
//! change; it never picks images. Transport: OpenRouter's decisions endpoint
//! (`POST https://openrouter.ai/api/alpha/decisions`, model `typesafe/jev-latest`), which takes the
//! same `{model, state, questions}` body and returns the same `answers` shape as TypeSafe's
//! native `/v1/systemone`.
//!
//! **Display intent (default, `DECIDE_MODE=intent`).** The question is not "was something picturable
//! mentioned?" but "is the presenter signalling that the audience should SEE something now?".
//! "I like watermelons" → no change; "here's what a watermelon looks like" → show. Jev gets two
//! questions in one call: `intent` (noul: yes/no probability) and `kind` (choice: new_render /
//! update / clear). Action = `kind` if P(intent) ≥ τ_intent, else no_change.
//! `DECIDE_MODE=topic` keeps the earlier single "did the topic change?" choice for A/B comparison.
//!
//! Fallbacks behind the same `ChangeDecision` output:
//! 1. Jev fails but a key exists → ask the chat query model for the action (doc §9 fallback).
//! 2. No key at all (offline dev/replay) → a transparent cue + vocabulary heuristic.

use ls_contracts::{Action, ChangeDecision, Displayed};
use serde::Deserialize;
use std::collections::HashMap;
use std::time::Duration;

/// OpenRouter base URL; override with OPENROUTER_BASE_URL (e.g. a local mock for latency tests).
fn base() -> String {
    std::env::var("OPENROUTER_BASE_URL").unwrap_or_else(|_| "https://openrouter.ai".into())
}
/// OpenRouter id for Jev on the decisions endpoint (`typesafe/jev-latest` does not exist there;
/// `~typesafe/jev-latest` is an alias of the same model).
pub const DEFAULT_JEV_MODEL: &str = "typesafe/jev-1.13";
/// Total time a decision may take, fallbacks included (Jev gets 700 ms of it).
pub const DECIDE_BUDGET: Duration = Duration::from_millis(1100);

const CONTEXT: &str = "A presenter is speaking live and one picture is on screen behind them. \
`displayed` is the picture now on screen and `displayed.trigger_text` is what they said when it went up. \
`prev` is their previous phrase and `curr` is what they are saying right now.";

const INTENT_Q: &str = "Is the presenter, in `curr`, signalling that the audience should now SEE something — \
directing attention to a picture, photo, graphic or chart (e.g. \"here's what a watermelon looks like\", \
\"take a look at\", \"picture this\", \"as you can see\", \"let me show you\", \"this is our office\"), or \
explicitly asking to change or clear what is shown? Merely mentioning or having an opinion about something \
(\"I like watermelons\", \"we talked about dogs\") is NOT a signal.";

const KIND_Q: &str = "If the presenter wants the audience to see something, what should happen to the picture?";

const TOPIC_Q: &str = "Should the picture change because of `curr`?";

/// Options for the `kind` question (intent mode).
pub fn kind_criteria() -> serde_json::Value {
    serde_json::json!({
        "new_render": "Show a different thing: they point the audience at a new subject",
        "update": "Refine what is on screen: same subject, different colour, angle, version or variant",
        "clear": "Take the picture away: they ask the audience to look back at them or to set the images aside"
    })
}

/// Options for the single choice (topic mode, and the LLM fallback).
pub fn criteria() -> serde_json::Value {
    serde_json::json!({
        "no_change": "No signal to show anything new: they are elaborating, giving an opinion, merely mentioning something, or saying filler",
        "new_render": "They point the audience at a new subject to look at (\"here's…\", \"take a look at…\", \"picture this…\")",
        "update": "They refine the subject on screen (a different colour, angle, version or variant of it)",
        "clear": "They ask the audience to look back at them or to set the images aside"
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Intent,
    Topic,
}

impl Mode {
    pub fn from_env() -> Self {
        match std::env::var("DECIDE_MODE").map(|v| v.to_lowercase()) {
            Ok(v) if v == "topic" => Mode::Topic,
            _ => Mode::Intent,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Jev,
    LlmFallback,
    Heuristic,
}

/// Extra detail for logs / the debug window.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Detail {
    /// P(display intent) when the intent question was asked.
    pub p_intent: Option<f32>,
}

#[derive(Clone)]
pub struct Decider {
    http: reqwest::Client,
    api_key: Option<String>,
    jev_model: String,
    chat_model: String,
    timeout: Duration,
    vocab: Vec<String>,
    pub mode: Mode,
    /// Minimum P(intent) to act (intent mode). DECIDE_INTENT_TAU, default 0.5 (live test: real requests 0.57–0.9, plain mentions ≤ 0.07).
    pub tau_intent: f32,
}

impl Decider {
    pub fn new(http: reqwest::Client, api_key: Option<String>, jev_model: Option<String>, chat_model: String, vocab: Vec<String>) -> Self {
        Self {
            http,
            api_key: api_key.filter(|k| !k.trim().is_empty()),
            jev_model: jev_model.unwrap_or_else(|| DEFAULT_JEV_MODEL.into()),
            chat_model,
            timeout: Duration::from_millis(700),
            vocab,
            mode: Mode::from_env(),
            tau_intent: std::env::var("DECIDE_INTENT_TAU").ok().and_then(|v| v.parse().ok()).unwrap_or(0.5),
        }
    }

    pub fn has_remote(&self) -> bool {
        self.api_key.is_some()
    }

    pub fn build_request(&self, prev: &str, curr: &str, d: &Displayed, now_ms: u64) -> serde_json::Value {
        let questions = match self.mode {
            Mode::Intent => serde_json::json!({
                "intent": {"type": "noul", "instructions": format!("{CONTEXT} {INTENT_Q}")},
                "kind": {"type": "choice", "instructions": format!("{CONTEXT} {KIND_Q}"), "criteria": kind_criteria()}
            }),
            Mode::Topic => serde_json::json!({
                "action": {"type": "choice", "instructions": format!("{CONTEXT} {TOPIC_Q}"), "criteria": criteria()}
            }),
        };
        serde_json::json!({ "model": self.jev_model, "state": state(prev, curr, d, now_ms), "questions": questions })
    }

    pub async fn warm_up(&self) {
        if self.api_key.is_some() {
            let blank = Displayed { image_id: None, caption: None, trigger_text: String::new(), shown_at_ms: 0 };
            let _ = tokio::time::timeout(Duration::from_secs(5), self.jev("", "hello", &blank, 0)).await;
        }
    }

    /// Never fails: on any error it degrades to the next fallback. Returns the source for logging.
    pub async fn decide(&self, chunk_id: u64, seq: u64, prev: &str, curr: &str, d: &Displayed, now_ms: u64) -> (ChangeDecision, Source) {
        let (dec, src, _) = self.decide_detailed(chunk_id, seq, prev, curr, d, now_ms).await;
        (dec, src)
    }

    pub async fn decide_detailed(&self, chunk_id: u64, seq: u64, prev: &str, curr: &str, d: &Displayed, now_ms: u64) -> (ChangeDecision, Source, Detail) {
        // Whole decision must land inside the stage's 1 s join window (measured: 900 + 900 ms
        // Jev-then-LLM timeouts made fallback answers arrive too late to be used).
        let started = std::time::Instant::now();
        if self.api_key.is_some() {
            match tokio::time::timeout(self.timeout, self.jev(prev, curr, d, now_ms)).await {
                Ok(Ok((action, p, detail))) => return (ChangeDecision { chunk_id, seq, action, p }, Source::Jev, detail),
                Ok(Err(e)) => eprintln!("decide: jev error: {e:#}"),
                Err(_) => eprintln!("decide: jev timed out after {:?}", self.timeout),
            }
            let left = DECIDE_BUDGET.saturating_sub(started.elapsed());
            if left >= Duration::from_millis(250) {
                match tokio::time::timeout(left, self.llm(prev, curr, d)).await {
                    Ok(Ok(action)) => {
                        return (ChangeDecision { chunk_id, seq, action, p: 0.7 }, Source::LlmFallback, Detail::default())
                    }
                    Ok(Err(e)) => eprintln!("decide: llm fallback error: {e:#}"),
                    Err(_) => eprintln!("decide: llm fallback timed out"),
                }
            }
        }
        let (action, p) = match self.mode {
            Mode::Intent => heuristic_intent(curr, d, &self.vocab),
            Mode::Topic => heuristic(curr, d, &self.vocab),
        };
        (ChangeDecision { chunk_id, seq, action, p }, Source::Heuristic, Detail::default())
    }

    async fn jev(&self, prev: &str, curr: &str, d: &Displayed, now_ms: u64) -> anyhow::Result<(Action, f32, Detail)> {
        let body = self.build_request(prev, curr, d, now_ms);
        let resp = self.http.post(format!("{}/api/alpha/decisions", base())).bearer_auth(self.api_key.as_deref().unwrap_or_default()).json(&body).send().await?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("HTTP {status}: {}", &text[..text.len().min(300)]);
        }
        match self.mode {
            Mode::Intent => parse_jev_intent(&text, self.tau_intent),
            Mode::Topic => parse_jev(&text).map(|(a, p)| (a, p, Detail::default())),
        }
    }

    async fn llm(&self, prev: &str, curr: &str, d: &Displayed) -> anyhow::Result<Action> {
        let prompt = format!(
            "{CONTEXT} {TOPIC_Q} Change ONLY if the presenter signals that the audience should see something; \
             a bare mention or opinion is no_change.\nOptions: {}\nState: {}\nReply with JSON only: {{\"action\": \"<option>\"}}",
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
        let resp = self.http.post(format!("{}/api/v1/chat/completions", base())).bearer_auth(self.api_key.as_deref().unwrap_or_default()).json(&body).send().await?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("HTTP {status}: {}", &text[..text.len().min(300)]);
        }
        let v: serde_json::Value = serde_json::from_str(&text)?;
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
    noul: Option<f32>,
}

/// Topic mode: parse `{answers: {action: {choice, probabilities, confidence}}}`. P = probability of
/// the chosen option (falls back to confidence).
pub fn parse_jev(body: &str) -> anyhow::Result<(Action, f32)> {
    let r: JevResponse = serde_json::from_str(body)?;
    let a = r.answers.get("action").ok_or_else(|| anyhow::anyhow!("no `action` answer in {body}"))?;
    let choice = a.choice.clone().ok_or_else(|| anyhow::anyhow!("no choice in {body}"))?;
    let p = a.probabilities.get(&choice).copied().or(a.confidence).unwrap_or(0.0);
    Ok((parse_action(&choice)?, p))
}

/// Intent mode: `{answers: {intent: {noul}, kind: {choice, probabilities}}}`.
/// Below τ_intent → (no_change, 1 − P(intent)); otherwise (kind, P(intent) · P(kind)).
pub fn parse_jev_intent(body: &str, tau_intent: f32) -> anyhow::Result<(Action, f32, Detail)> {
    let r: JevResponse = serde_json::from_str(body)?;
    let p_intent = r
        .answers
        .get("intent")
        .and_then(|a| a.noul)
        .ok_or_else(|| anyhow::anyhow!("no `intent` noul in {body}"))?;
    let detail = Detail { p_intent: Some(p_intent) };
    if p_intent < tau_intent {
        return Ok((Action::NoChange, 1.0 - p_intent, detail));
    }
    let k = r.answers.get("kind").ok_or_else(|| anyhow::anyhow!("no `kind` answer in {body}"))?;
    let choice = k.choice.clone().ok_or_else(|| anyhow::anyhow!("no kind choice in {body}"))?;
    let p_kind = k.probabilities.get(&choice).copied().or(k.confidence).unwrap_or(0.0);
    Ok((parse_action(&choice)?, p_intent * p_kind, detail))
}

pub use ls_query::{after_last_cue, CUES};
const REFINE_CUES: &[&str] = &["actually", "make that", "instead", "the other one", "a different", "in red", "in blue"];

fn subject_of(caption: &str) -> String {
    caption.split('(').next().unwrap_or(caption).trim().to_lowercase()
}

fn mentions(words: &[String], subject: &str) -> bool {
    let parts: Vec<&str> = subject.split(' ').filter(|s| s.len() > 2).collect();
    words.iter().any(|w| {
        let w = w.trim_end_matches('s');
        subject == w || parts.iter().any(|p| p.len() > 3 && p.trim_end_matches('s') == w)
    })
}

/// Offline intent heuristic (no API key): change only when a presentational cue is followed, within a
/// few words, by a library subject that is not already on screen. A bare mention is no_change.
pub fn heuristic_intent(curr: &str, d: &Displayed, vocab: &[String]) -> (Action, f32) {
    let Some(words) = after_last_cue(curr, 10) else {
        return (Action::NoChange, 0.9);
    };
    let on_screen = d.caption.clone().map(|c| subject_of(&c)).unwrap_or_default();
    let refine = REFINE_CUES.iter().any(|c| curr.to_lowercase().contains(c));
    // Best-matching library subject: most of its words present ("white rose" beats "red rose" for
    // "here's the white rose"), ties → longer subject.
    let best = vocab
        .iter()
        .map(|cap| subject_of(cap))
        .filter(|s| s.len() >= 3 && mentions(&words, s))
        .map(|s| (match_score(&words, &s), s))
        .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap().then(a.1.len().cmp(&b.1.len())));
    let Some((_, subject)) = best else {
        return (Action::NoChange, 0.9);
    };
    if subject == on_screen {
        return (Action::NoChange, 0.9);
    }
    let shares = !on_screen.is_empty() && subject.split(' ').any(|p| p.len() > 3 && on_screen.contains(p));
    if refine && shares { (Action::Update, 0.8) } else { (Action::NewRender, 0.85) }
}

/// Fraction of a subject's words (len > 2) present in `words`.
fn match_score(words: &[String], subject: &str) -> f32 {
    let parts: Vec<&str> = subject.split(' ').filter(|s| s.len() > 2).collect();
    if parts.is_empty() {
        return 0.0;
    }
    let hit = parts.iter().filter(|p| words.iter().any(|w| w.trim_end_matches('s') == p.trim_end_matches('s'))).count();
    hit as f32 / parts.len() as f32
}

/// Topic-mode heuristic (DECIDE_MODE=topic): change when the newest speech names a library subject
/// that is not what's on screen, cue or not.
pub fn heuristic(curr: &str, d: &Displayed, vocab: &[String]) -> (Action, f32) {
    let lower = curr.to_lowercase();
    let on_screen = d.caption.clone().unwrap_or_default().to_lowercase();
    for cap in vocab {
        let subject = subject_of(cap);
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
    fn decider(mode: Mode) -> Decider {
        let mut d = Decider::new(reqwest::Client::new(), Some("k".into()), None, "m".into(), vec![]);
        d.mode = mode;
        d
    }

    #[test]
    fn intent_request_asks_two_questions() {
        let v = decider(Mode::Intent).build_request("prev", "curr", &disp(Some("eagle")), 6000);
        assert_eq!(v["model"], DEFAULT_JEV_MODEL);
        assert_eq!(v["state"]["displayed"]["caption"], "eagle");
        assert_eq!(v["state"]["displayed"]["seconds_on_screen"], 5);
        assert_eq!(v["questions"]["intent"]["type"], "noul");
        assert!(v["questions"]["intent"].get("criteria").is_none(), "OpenRouter needs both true/false if criteria present");
        assert_eq!(v["questions"]["kind"]["type"], "choice");
        assert_eq!(v["questions"]["kind"]["criteria"].as_object().unwrap().len(), 3);
    }

    #[test]
    fn topic_request_keeps_the_single_choice() {
        let v = decider(Mode::Topic).build_request("p", "c", &disp(None), 0);
        assert_eq!(v["questions"]["action"]["type"], "choice");
        assert_eq!(v["questions"]["action"]["criteria"].as_object().unwrap().len(), 4);
    }

    #[test]
    fn parses_intent_response() {
        let body = r#"{"model":"jev-1.13.0","answers":{"intent":{"type":"noul","noul":0.9},"kind":{"type":"choice","choice":"new_render","confidence":0.8,"probabilities":{"new_render":0.8,"update":0.15,"clear":0.05}}}}"#;
        let (a, p, d) = parse_jev_intent(body, 0.6).unwrap();
        assert_eq!(a, Action::NewRender);
        assert!((p - 0.72).abs() < 1e-5);
        assert_eq!(d.p_intent, Some(0.9));
        let low = r#"{"answers":{"intent":{"type":"noul","noul":0.2},"kind":{"type":"choice","choice":"new_render","probabilities":{"new_render":0.9}}}}"#;
        let (a, p, _) = parse_jev_intent(low, 0.6).unwrap();
        assert_eq!(a, Action::NoChange);
        assert!((p - 0.8).abs() < 1e-5);
    }

    #[test]
    fn parses_topic_response() {
        let body = r#"{"id":"gen-1","model":"jev-1.13.0","provider":"TypeSafe","answers":{"action":{"type":"choice","choice":"new_render","confidence":0.8,"probabilities":{"no_change":0.1,"new_render":0.85,"update":0.03,"clear":0.02}}},"usage":{"input_tokens":312,"output_tokens":48,"cost":0.00001}}"#;
        assert_eq!(parse_jev(body).unwrap(), (Action::NewRender, 0.85));
        let conf = r#"{"answers":{"action":{"type":"choice","choice":"no_change","confidence":0.7}}}"#;
        assert_eq!(parse_jev(conf).unwrap(), (Action::NoChange, 0.7));
    }

    #[test]
    fn intent_heuristic_needs_a_cue() {
        let vocab = vec!["watermelon (fruit)".to_string(), "red rose (flowers)".to_string(), "white rose (flowers)".to_string()];
        assert_eq!(heuristic_intent("honestly I like watermelons a lot", &disp(None), &vocab).0, Action::NoChange);
        assert_eq!(heuristic_intent("ok so here's what a watermelon looks like", &disp(None), &vocab).0, Action::NewRender);
        assert_eq!(heuristic_intent("Take a look at this watermelon", &disp(Some("watermelon (fruit)")), &vocab).0, Action::NoChange);
        assert_eq!(heuristic_intent("here's a red rose", &disp(None), &vocab).0, Action::NewRender);
        assert_eq!(heuristic_intent("actually, here's the white rose instead", &disp(Some("red rose (flowers)")), &vocab).0, Action::Update);
        // A cue far from the subject does not count.
        assert_eq!(
            heuristic_intent("here's the thing about our company culture and how we hire people, we like watermelon", &disp(None), &vocab).0,
            Action::NoChange
        );
    }

    #[test]
    fn topic_heuristic_changes_on_mention() {
        let vocab = vec!["eagle (animals)".to_string(), "guitar (instruments)".to_string()];
        assert_eq!(heuristic("and then an eagle flew over", &disp(None), &vocab).0, Action::NewRender);
        assert_eq!(heuristic("the eagle again", &disp(Some("eagle (animals)")), &vocab).0, Action::NoChange);
        assert_eq!(heuristic("I play guitars", &disp(Some("eagle (animals)")), &vocab).0, Action::NewRender);
    }

    #[tokio::test]
    async fn no_key_is_heuristic() {
        let mut dcd = Decider::new(reqwest::Client::new(), None, None, "m".into(), vec!["zebra (animals)".into()]);
        dcd.mode = Mode::Intent;
        let (d, src) = dcd.decide(3, 9, "", "look at that zebra", &disp(None), 0).await;
        assert_eq!(src, Source::Heuristic);
        assert_eq!((d.chunk_id, d.seq, d.action), (3, 9, Action::NewRender));
        let (d, _) = dcd.decide(4, 10, "", "zebras are my favourite", &disp(None), 0).await;
        assert_eq!(d.action, Action::NoChange);
    }
}
