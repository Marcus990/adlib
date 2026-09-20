//! Shared pipeline types: what Hear emits (`Chunk`), a photo-search hit (`Match`) and what the web view is
//! told to render (`RenderEvent`). (The Jev / phrase-model / stage types were removed with the Luna refactor.)

use serde::{Deserialize, Serialize};

/// A transcript chunk from Track A (Hear). `is_final == false` means `curr`
/// (the in-progress chunk, re-transcribed ~every 0.5–1 s); `true` means a finalised chunk (`prev`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Chunk {
    pub id: u64,
    pub text: String,
    /// The leading words of `text` that the previous decode of this same audio also produced.
    /// Whisper re-transcribes the whole utterance every tick, so agreement across decodes is a
    /// confidence signal: a real word survives a re-decode, a hallucinated one does not.
    #[serde(default)]
    pub stable: String,
    pub t_start_ms: u64,
    pub t_end_ms: u64,
    pub is_final: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Match {
    pub chunk_id: u64,
    pub image_id: String,
    pub caption: String,
    pub score: f32,
    pub phrase: String,
}

// ---- Rust → web view ----

/// Normalised 0..1 rectangle; v1 is always full screen.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const FULL: Rect = Rect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 };
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RenderEvent {
    /// "render" | "update" | "clear"
    pub kind: &'static str,
    pub image_id: Option<String>,
    /// img://<id>, served from the LRU cache
    pub url: Option<String>,
    pub rect: Rect,
    pub chunk_id: u64,
    pub ts_ms: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_event_json_shape() {
        let ev = RenderEvent {
            kind: "render",
            image_id: Some("tokyo".into()),
            url: Some("img://tokyo".into()),
            rect: Rect::FULL,
            chunk_id: 7,
            ts_ms: 1234,
        };
        let v: serde_json::Value = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["kind"], "render");
        assert_eq!(v["rect"]["w"], 1.0);
        assert_eq!(v["chunk_id"], 7);
    }

    #[test]
    fn chunk_roundtrips() {
        let c = Chunk { id: 1, text: "hello".into(), stable: String::new(), t_start_ms: 0, t_end_ms: 900, is_final: false };
        let s = serde_json::to_string(&c).unwrap();
        assert_eq!(serde_json::from_str::<Chunk>(&s).unwrap(), c);
    }
}
