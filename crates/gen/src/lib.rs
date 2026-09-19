//! Image generation fallback (ASSETS_HANDOFF §6, AS11/AS12): when the presenter names something the photo
//! library cannot show, draw it instead. SDXL-Lightning (4 steps) on Baseten, deployed by Marcus.
//!
//! Measured 2026-09-19 from this Mac: **1.1–1.3 s at 512 px, ~2.0 s at 768 px warm; 146 s on a cold start**
//! (the deployment scales to zero), which is why [`ImageGen::warm_up`] runs in the background at launch.
//! Bare prompts gave junk ("planet Earth from space" → colour noise); [`PROMPT`] fixed that.

use anyhow::{Context, Result};
use base64::Engine as _;
use std::time::Duration;

/// Marcus's deployment; override with BASETEN_URL.
pub const DEFAULT_URL: &str = "https://model-3yvmgyn3.api.baseten.co/deployment/qrmdkv1/predict";
/// SDXL-Lightning needs the style spelled out at 4 steps.
pub const PROMPT: &str = "{}, high quality photograph, sharp focus, natural colors, plain background";
/// Things a diffusion model renders badly — the icon library or nothing (AS14).
const NEVER: &[&str] = &["logo", "icon", "wordmark", "brand", "screenshot", "chart", "graph", "diagram", "text", "slide", "ui", "interface"];
/// Subjects too vague to draw — usually a mis-transcription ("a single scene", 09-19).
const VAGUE: &[&str] = &["scene", "thing", "things", "picture", "image", "photo", "stuff", "view", "example", "idea", "part", "way", "moment", "point"];

#[derive(Clone)]
pub struct ImageGen {
    http: reqwest::Client,
    api_key: Option<String>,
    url: String,
    pub size: u32,
    timeout: Duration,
}

impl ImageGen {
    pub fn new(http: reqwest::Client, api_key: Option<String>, url: Option<String>, size: Option<u32>) -> Self {
        Self {
            http,
            api_key: api_key.filter(|k| !k.trim().is_empty()),
            url: url.unwrap_or_else(|| DEFAULT_URL.into()),
            size: size.unwrap_or(768),
            timeout: Duration::from_secs(8),
        }
    }

    pub fn enabled(&self) -> bool {
        self.api_key.is_some()
    }

    /// True when a diffusion model is the wrong tool for this subject.
    pub fn refuses(subject: &str) -> bool {
        let s = subject.to_lowercase();
        let words: Vec<&str> = s.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
        NEVER.iter().any(|w| words.contains(w))
            // "a scene" is nothing to draw; "a scene from a film" is (the vague word is the whole subject)
            || words.iter().all(|w| VAGUE.contains(w) || ["a", "an", "the", "of", "single", "our", "this", "that"].contains(w))
    }

    /// JPEG bytes for a subject, or None when generation is off, refused, or too slow.
    pub async fn generate(&self, subject: &str) -> Option<Vec<u8>> {
        if !self.enabled() || Self::refuses(subject) || subject.trim().is_empty() {
            return None;
        }
        match tokio::time::timeout(self.timeout, self.post(&PROMPT.replace("{}", subject.trim()))).await {
            Ok(Ok(bytes)) => Some(bytes),
            Ok(Err(e)) => {
                eprintln!("image gen: {e:#}");
                None
            }
            Err(_) => {
                eprintln!("image gen: timed out after {:?} (cold start?)", self.timeout);
                None
            }
        }
    }

    /// Wake the deployment so the first real request isn't a 146 s cold start. Fire and forget.
    pub async fn warm_up(&self) {
        if self.enabled() {
            let _ = tokio::time::timeout(Duration::from_secs(200), self.post(&PROMPT.replace("{}", "a gray stone"))).await;
        }
    }

    async fn post(&self, prompt: &str) -> Result<Vec<u8>> {
        let body = serde_json::json!({"prompt": prompt, "width": self.size, "height": self.size});
        let resp = self
            .http
            .post(&self.url)
            .header("Authorization", format!("Api-Key {}", self.api_key.as_deref().unwrap_or_default()))
            .json(&body)
            .send()
            .await?;
        let status = resp.status();
        let text = resp.text().await?;
        anyhow::ensure!(status.is_success(), "HTTP {status}: {}", &text[..text.len().min(200)]);
        let v: serde_json::Value = serde_json::from_str(&text)?;
        let b64 = v["result"].as_str().context("no `result` in the response")?;
        Ok(base64::engine::general_purpose::STANDARD.decode(b64)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_what_diffusion_renders_badly() {
        assert!(ImageGen::refuses("the Baseten logo"));
        assert!(ImageGen::refuses("a bar chart of users"));
        assert!(ImageGen::refuses("a screenshot of the app"));
        assert!(!ImageGen::refuses("an owl"));
        assert!(!ImageGen::refuses("a sunflower in a field"));
        assert!(!ImageGen::refuses("planet earth from space"));
        assert!(ImageGen::refuses("a single scene"), "too vague to draw");
        assert!(ImageGen::refuses("the thing"));
        assert!(!ImageGen::refuses("a scene from a film"));
    }

    #[test]
    fn disabled_without_a_key() {
        let g = ImageGen::new(reqwest::Client::new(), None, None, None);
        assert!(!g.enabled());
    }
}
