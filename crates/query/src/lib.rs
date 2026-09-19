//! Track D: the fast query model (design doc §7.2). Turns prev + curr (+ what is on screen) into
//! 1–3 short visual noun phrases. Remote: an OpenRouter chat model with JSON output and a hard
//! 700 ms timeout. Fallback: local noun-phrase extraction, so this branch never blocks.

use ls_contracts::{Displayed, QueryResult};
use serde::Deserialize;
use std::time::Duration;

pub const DEFAULT_MODEL: &str = "google/gemini-2.5-flash-lite";
/// OpenRouter base URL; override with OPENROUTER_BASE_URL (e.g. a local mock for latency tests).
fn base() -> String {
    std::env::var("OPENROUTER_BASE_URL").unwrap_or_else(|_| "https://openrouter.ai".into())
}

const SYSTEM_PROMPT: &str = "You turn a live presenter's speech into image search phrases for a local photo library. \
Return JSON only: {\"phrases\": [...]} with 1 to 3 short, concrete, visual noun phrases (2-5 words each), most important first. \
Focus on the NEWEST speech. Drop filler words. Resolve references using what is on screen \
(\"make it red\" with a car on screen -> \"red car\"). If the talk is abstract, name the most picturable concrete thing mentioned. \
Prefer words used in the library list when they fit.";

#[derive(Clone)]
pub struct QueryClient {
    http: reqwest::Client,
    api_key: Option<String>,
    model: String,
    timeout: Duration,
    /// Library captions: shown to the model and used to bias the local fallback.
    vocab: Vec<String>,
}

impl QueryClient {
    pub fn new(http: reqwest::Client, api_key: Option<String>, model: Option<String>, vocab: Vec<String>) -> Self {
        Self {
            http,
            api_key: api_key.filter(|k| !k.trim().is_empty()),
            model: model.unwrap_or_else(|| DEFAULT_MODEL.to_string()),
            timeout: Duration::from_millis(700),
            vocab,
        }
    }

    pub fn has_remote(&self) -> bool {
        self.api_key.is_some()
    }

    /// Never fails and never takes longer than the timeout (plus local fallback time).
    pub async fn query(&self, chunk_id: u64, prev: &str, curr: &str, displayed: &Displayed) -> QueryResult {
        if self.api_key.is_some() {
            match tokio::time::timeout(self.timeout, self.remote(prev, curr, displayed)).await {
                Ok(Ok(phrases)) if !phrases.is_empty() => {
                    return QueryResult { chunk_id, phrases, from_fallback: false };
                }
                Ok(Err(e)) => eprintln!("query: remote error: {e:#}"),
                Ok(Ok(_)) => eprintln!("query: remote returned no phrases"),
                Err(_) => eprintln!("query: remote timed out after {:?}", self.timeout),
            }
        }
        let text = if curr.trim().is_empty() { prev } else { curr };
        QueryResult { chunk_id, phrases: fallback_phrases(text, &self.vocab), from_fallback: true }
    }

    /// Warm the HTTPS connection (TLS + HTTP/2) before the talk.
    pub async fn warm_up(&self) {
        if self.api_key.is_some() {
            let _ = tokio::time::timeout(Duration::from_secs(5), self.remote("", "hello", &blank())).await;
        }
    }

    async fn remote(&self, prev: &str, curr: &str, displayed: &Displayed) -> anyhow::Result<Vec<String>> {
        let key = self.api_key.as_deref().unwrap_or_default();
        let on_screen = displayed.caption.as_deref().unwrap_or("nothing");
        let library = if self.vocab.is_empty() { String::new() } else { format!("\nLibrary: {}", self.vocab.join("; ")) };
        let user = format!("On screen: {on_screen}\nPrevious speech: {prev}\nNewest speech: {curr}{library}");
        let body = serde_json::json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": SYSTEM_PROMPT},
                {"role": "user", "content": user}
            ],
            "response_format": {"type": "json_object"},
            "temperature": 0,
            "max_tokens": 60,
            "provider": {"sort": "latency"}
        });
        let resp = self.http.post(format!("{}/api/v1/chat/completions", base())).bearer_auth(key).json(&body).send().await?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("HTTP {status}: {}", truncate(&text, 300));
        }
        parse_chat_phrases(&text)
    }
}

fn blank() -> Displayed {
    Displayed { image_id: None, caption: None, trigger_text: String::new(), shown_at_ms: 0 }
}

fn truncate(s: &str, n: usize) -> &str {
    &s[..s.char_indices().nth(n).map(|(i, _)| i).unwrap_or(s.len())]
}

#[derive(Deserialize)]
struct Chat {
    choices: Vec<ChatChoice>,
}
#[derive(Deserialize)]
struct ChatChoice {
    message: ChatMsg,
}
#[derive(Deserialize)]
struct ChatMsg {
    content: Option<String>,
}
#[derive(Deserialize)]
struct Phrases {
    phrases: Vec<String>,
}

/// Parse an OpenRouter chat-completions response whose content is `{"phrases": [...]}`
/// (tolerates code fences around the JSON).
pub fn parse_chat_phrases(body: &str) -> anyhow::Result<Vec<String>> {
    let chat: Chat = serde_json::from_str(body)?;
    let content = chat.choices.first().and_then(|c| c.message.content.clone()).unwrap_or_default();
    let start = content.find('{').ok_or_else(|| anyhow::anyhow!("no JSON object in: {content}"))?;
    let end = content.rfind('}').ok_or_else(|| anyhow::anyhow!("no JSON object in: {content}"))?;
    let p: Phrases = serde_json::from_str(&content[start..=end])?;
    Ok(p.phrases.into_iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).take(3).collect())
}

const STOP: &[&str] = &[
    "a", "an", "the", "and", "or", "but", "so", "um", "uh", "like", "you", "know", "i", "we", "our", "us", "my", "me", "it",
    "its", "it's", "is", "are", "was", "were", "be", "been", "being", "this", "that", "these", "those", "there", "here",
    "to", "of", "in", "on", "at", "for", "with", "about", "from", "by", "as", "into", "over", "next", "then", "now",
    "just", "really", "very", "also", "let's", "lets", "let", "talk", "tell", "want", "going", "gonna", "get", "got",
    "have", "has", "had", "do", "does", "did", "can", "could", "will", "would", "should", "what", "which", "who", "how",
    "why", "when", "where", "all", "some", "any", "one", "two", "first", "second", "okay", "ok", "right", "yeah", "well",
    "think", "see", "look", "show", "make", "say", "said", "thing", "things", "stuff", "today", "they", "them", "their",
    "he", "she", "his", "her", "him", "not", "no", "yes", "if", "because", "every", "single", "whole", "up", "out",
    "more", "most", "much", "many", "lot", "lots", "kind", "sort", "bit", "little", "new", "good", "great", "big",
    "sits", "sit", "right", "next", "i'm", "we're", "you're", "that's", "what's", "here's", "there's", "don't",
];

/// Local noun-phrase fallback: runs of content words, newest first, preferring runs that contain
/// a library vocabulary word. Crude but instant and offline.
pub fn fallback_phrases(text: &str, vocab: &[String]) -> Vec<String> {
    let words: Vec<String> = text
        .split(|c: char| !(c.is_alphanumeric() || c == '\'' || c == '-'))
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase())
        .collect();
    let mut runs: Vec<Vec<String>> = vec![];
    let mut cur: Vec<String> = vec![];
    for w in words {
        let content = !STOP.contains(&w.as_str()) && w.len() > 2 && !w.chars().all(|c| c.is_ascii_digit());
        if content {
            cur.push(w);
        } else if !cur.is_empty() {
            runs.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        runs.push(cur);
    }
    let vocab_words: Vec<String> = vocab
        .iter()
        .flat_map(|c| c.split(|ch: char| !ch.is_alphanumeric()).map(|w| w.to_lowercase()).collect::<Vec<_>>())
        .filter(|w| w.len() > 2)
        .collect();
    // Newest first; keep at most 4 words of each run (the tail, nearest the head noun).
    let mut cands: Vec<(bool, usize, String)> = runs
        .iter()
        .enumerate()
        .rev()
        .map(|(i, r)| {
            let tail = &r[r.len().saturating_sub(4)..];
            let hit = tail.iter().any(|w| vocab_words.iter().any(|v| v == w || v.trim_end_matches('s') == w.trim_end_matches('s')));
            (hit, i, tail.join(" "))
        })
        .collect();
    // Stable sort: vocabulary hits first, recency preserved within each group.
    cands.sort_by_key(|(hit, _, _)| !*hit);
    let mut out: Vec<String> = vec![];
    for (_, _, p) in cands {
        if !out.contains(&p) {
            out.push(p);
        }
        if out.len() == 3 {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_extracts_content_runs_newest_first() {
        let p = fallback_phrases("So, um, today I want to tell you about our new office in Tokyo, and the river at night", &[]);
        assert_eq!(p, vec!["night", "river", "tokyo"], "newest content runs first");
    }

    #[test]
    fn fallback_prefers_library_vocabulary() {
        let vocab = vec!["eagle (animals)".to_string(), "guitar (instruments)".to_string()];
        let p = fallback_phrases("my grandfather played the guitar every weekend in the garden", &vocab);
        assert_eq!(p[0], "guitar", "got {p:?}");
        assert!(p.len() >= 2);
    }

    #[test]
    fn fallback_empty_text() {
        assert!(fallback_phrases("um, so, yeah", &[]).is_empty());
    }

    #[test]
    fn parses_openrouter_chat_json() {
        let body = r#"{"id":"x","choices":[{"message":{"role":"assistant","content":"```json\n{\"phrases\": [\"Tokyo office skyline\", \"night city lights\"]}\n```"}}]}"#;
        assert_eq!(parse_chat_phrases(body).unwrap(), vec!["Tokyo office skyline", "night city lights"]);
    }

    #[tokio::test]
    async fn no_key_uses_fallback_immediately() {
        let q = QueryClient::new(reqwest::Client::new(), None, None, vec![]);
        let d = blank();
        let r = q.query(7, "", "the golden eagle over the mountains", &d).await;
        assert!(r.from_fallback);
        assert_eq!(r.chunk_id, 7);
        assert!(r.phrases.contains(&"golden eagle".to_string()), "got {:?}", r.phrases);
    }
}
