//! Shared pipeline contracts (design doc §8, "Hour 0: contracts every agent builds against").
//!
//! These types are the integration boundary between the five tracks. Field names and
//! shapes follow the design doc exactly; only derives were added (Debug/Clone/PartialEq
//! everywhere, Deserialize where replay and logging need to read them back).
//! Any change here must be recorded in PROGRESS.md and applied to every consumer.

use serde::{Deserialize, Serialize};

/// A transcript chunk from Track A (Hear). `is_final == false` means `curr`
/// (the in-progress chunk, re-transcribed ~every 0.5–1 s); `true` means a finalised chunk (`prev`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Chunk {
    pub id: u64,
    pub text: String,
    pub t_start_ms: u64,
    pub t_end_ms: u64,
    pub is_final: bool,
}

/// What is on screen (or pending), including the transcript that triggered it (§7.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Displayed {
    pub image_id: Option<String>,
    pub caption: Option<String>,
    pub trigger_text: String,
    pub shown_at_ms: u64,
    /// Canvas mode: one line per tile on the board (photos, charts with values, diagrams with steps),
    /// so Jev can tell when the talk is already illustrated. `caption` stays the focused photo.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub on_screen: Vec<String>,
}

// ---- Branch 1: Jev (whether) ----

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    #[default]
    NoChange,
    NewRender,
    Update,
    Clear,
}

/// Which path owns this sentence. Jev routes; the pipeline dispatches. One sentence, one visual — before
/// this, a sentence with numbers drew a chart *and* generated pictures of "first year" and "200 users".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Visual {
    /// A concrete thing to look at → library photo, or generated when the library has none.
    Photo,
    /// Quantities → the canvas agent draws a chart.
    Chart,
    /// Steps, cycles, parts, cause and effect → the canvas agent draws a diagram.
    Diagram,
    /// About what is already on screen: compare, zoom, point at it, clear, remove.
    Board,
    #[default]
    None,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ChangeDecision {
    pub chunk_id: u64,
    pub seq: u64,
    pub action: Action,
    pub p: f32,
    /// Jev's routing answer and its probability (0 when Jev was unavailable).
    #[serde(default)]
    pub visual: Visual,
    #[serde(default)]
    pub p_visual: f32,
}

// ---- Branch 2: query model + search (what) ----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueryResult {
    pub chunk_id: u64,
    pub phrases: Vec<String>,
    pub from_fallback: bool,
    /// The speech named one library subject outright, so the phrase model was skipped (≈0.45 s saved).
    #[serde(default)]
    pub named: bool,
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
    fn action_serializes_snake_case() {
        assert_eq!(serde_json::to_string(&Action::NewRender).unwrap(), "\"new_render\"");
        assert_eq!(serde_json::from_str::<Action>("\"no_change\"").unwrap(), Action::NoChange);
    }

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
        let c = Chunk { id: 1, text: "hello".into(), t_start_ms: 0, t_end_ms: 900, is_final: false };
        let s = serde_json::to_string(&c).unwrap();
        assert_eq!(serde_json::from_str::<Chunk>(&s).unwrap(), c);
    }
}
