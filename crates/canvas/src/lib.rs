//! Canvas mode (CANVAS.md): an evolving board of ≤ 4 library images plus ≤ 3 annotations.
//! Pure and deterministic. The fast path (stage renders) and the agent (tool calls) both mutate the
//! board through [`Canvas`]; every mutation bumps `version` so stale agent answers can be dropped.

use serde::{Deserialize, Serialize};

pub const MAX_ELEMENTS: usize = 4;
pub const MAX_ANNOTATIONS: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Element {
    pub id: String,
    pub image_id: String,
    pub caption: String,
    pub url: String,
    pub rect: Rect,
    pub z: u32,
    pub focus: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnnotationKind {
    Highlight,
    Frame,
    Arrow,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Annotation {
    pub id: String,
    pub kind: AnnotationKind,
    pub targets: Vec<String>,
    pub label: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Layout {
    #[default]
    Auto,
    Hero,
    Compare,
    Grid,
}

/// What the web view renders. Sent whole on every change (small).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Scene {
    pub version: u64,
    pub layout: Layout,
    pub elements: Vec<Element>,
    pub annotations: Vec<Annotation>,
    /// Why it changed ("fast:render", "agent:arrange(compare)", …) — for the debug window.
    pub reason: String,
    pub chunk_id: u64,
}

/// One validated agent operation (tool call).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    Focus { id: String },
    Remove { id: String },
    Arrange { layout: Layout },
    Annotate { kind: AnnotationKind, targets: Vec<String>, label: Option<String> },
    ClearAnnotations,
    ClearBoard,
}

#[derive(Debug, Default)]
pub struct Canvas {
    scene: Scene,
    next_id: u64,
}

impl Canvas {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn scene(&self) -> &Scene {
        &self.scene
    }

    fn bump(&mut self, reason: String, chunk_id: u64) -> Scene {
        self.scene.version += 1;
        self.scene.reason = reason;
        self.scene.chunk_id = chunk_id;
        self.relayout();
        self.scene.clone()
    }

    fn new_id(&mut self, prefix: &str) -> String {
        self.next_id += 1;
        format!("{prefix}{}", self.next_id)
    }

    // ---- fast path (from stage renders) ----

    /// New image: add it, focus it, auto layout; evict the oldest past the cap.
    pub fn render(&mut self, image_id: &str, caption: &str, url: &str, chunk_id: u64) -> Scene {
        if let Some(e) = self.scene.elements.iter_mut().find(|e| e.image_id == image_id) {
            // Already on the board: just bring it into focus.
            let id = e.id.clone();
            return self.focus_inner(&id, format!("fast:refocus {image_id}"), chunk_id);
        }
        let id = self.new_id("e");
        let z = self.scene.elements.iter().map(|e| e.z).max().unwrap_or(0) + 1;
        for e in self.scene.elements.iter_mut() {
            e.focus = false;
        }
        self.scene.elements.push(Element {
            id,
            image_id: image_id.into(),
            caption: caption.into(),
            url: url.into(),
            rect: Rect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 },
            z,
            focus: true,
        });
        while self.scene.elements.len() > MAX_ELEMENTS {
            let old = self.scene.elements.remove(0);
            self.scene.annotations.retain(|a| !a.targets.contains(&old.id));
        }
        self.scene.layout = Layout::Auto;
        self.bump(format!("fast:render {image_id}"), chunk_id)
    }

    /// Refinement: replace the focused image in place (keeps its position and id).
    pub fn update(&mut self, image_id: &str, caption: &str, url: &str, chunk_id: u64) -> Scene {
        match self.scene.elements.iter_mut().find(|e| e.focus) {
            Some(e) => {
                e.image_id = image_id.into();
                e.caption = caption.into();
                e.url = url.into();
                self.bump(format!("fast:update {image_id}"), chunk_id)
            }
            None => self.render(image_id, caption, url, chunk_id),
        }
    }

    pub fn clear(&mut self, chunk_id: u64) -> Scene {
        self.scene.elements.clear();
        self.scene.annotations.clear();
        self.scene.layout = Layout::Auto;
        self.bump("fast:clear".into(), chunk_id)
    }

    // ---- agent path ----

    /// Apply validated ops iff the scene is still at `expected_version` (else the agent reasoned
    /// about a stale board). Returns the new scene if anything changed.
    pub fn apply(&mut self, expected_version: u64, ops: &[Op], chunk_id: u64) -> Option<Scene> {
        if self.scene.version != expected_version || ops.is_empty() {
            return None;
        }
        let mut applied = vec![];
        for op in ops {
            if self.apply_one(op) {
                applied.push(op_name(op));
            }
        }
        if applied.is_empty() {
            return None;
        }
        Some(self.bump(format!("agent:{}", applied.join(", ")), chunk_id))
    }

    fn has(&self, id: &str) -> bool {
        self.scene.elements.iter().any(|e| e.id == id)
    }

    fn apply_one(&mut self, op: &Op) -> bool {
        match op {
            Op::Focus { id } if self.has(id) => {
                for e in self.scene.elements.iter_mut() {
                    e.focus = &e.id == id;
                }
                true
            }
            Op::Remove { id } if self.has(id) => {
                self.scene.elements.retain(|e| &e.id != id);
                self.scene.annotations.retain(|a| !a.targets.contains(id));
                if !self.scene.elements.iter().any(|e| e.focus) {
                    if let Some(e) = self.scene.elements.last_mut() {
                        e.focus = true;
                    }
                }
                true
            }
            Op::Arrange { layout } if *layout != self.scene.layout => {
                self.scene.layout = *layout;
                true
            }
            Op::Annotate { kind, targets, label } => {
                let targets: Vec<String> = targets.iter().filter(|t| self.has(t)).cloned().collect();
                if targets.is_empty() || (*kind == AnnotationKind::Arrow && targets.len() < 2 && self.scene.elements.len() < 2) {
                    return false;
                }
                let id = self.new_id("a");
                self.scene.annotations.push(Annotation {
                    id,
                    kind: *kind,
                    targets,
                    label: label.clone().map(|l| l.chars().take(40).collect()),
                });
                while self.scene.annotations.len() > MAX_ANNOTATIONS {
                    self.scene.annotations.remove(0);
                }
                true
            }
            Op::ClearAnnotations if !self.scene.annotations.is_empty() => {
                self.scene.annotations.clear();
                true
            }
            Op::ClearBoard if !self.scene.elements.is_empty() => {
                self.scene.elements.clear();
                self.scene.annotations.clear();
                self.scene.layout = Layout::Auto;
                true
            }
            _ => false,
        }
    }

    fn focus_inner(&mut self, id: &str, reason: String, chunk_id: u64) -> Scene {
        for e in self.scene.elements.iter_mut() {
            e.focus = e.id == id;
        }
        self.bump(reason, chunk_id)
    }

    // ---- layout engine ----

    fn relayout(&mut self) {
        let n = self.scene.elements.len();
        if n == 0 {
            return;
        }
        let focus = self.scene.elements.iter().position(|e| e.focus).unwrap_or(n - 1);
        let rects = layout_rects(self.scene.layout, n, focus);
        for (e, r) in self.scene.elements.iter_mut().zip(rects) {
            e.rect = r;
        }
    }
}

fn op_name(op: &Op) -> String {
    match op {
        Op::Focus { id } => format!("focus({id})"),
        Op::Remove { id } => format!("remove({id})"),
        Op::Arrange { layout } => format!("arrange({layout:?})").to_lowercase(),
        Op::Annotate { kind, .. } => format!("annotate({kind:?})").to_lowercase(),
        Op::ClearAnnotations => "clear_annotations".into(),
        Op::ClearBoard => "clear_board".into(),
    }
}

const G: f32 = 0.02; // gutter between tiles (fraction of screen)

fn r(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect { x, y, w, h }
}

/// Rects (0..1) for `n` elements in element order. `focus` is the index of the focused element.
pub fn layout_rects(layout: Layout, n: usize, focus: usize) -> Vec<Rect> {
    let hero = |n: usize, focus: usize| -> Vec<Rect> {
        // Focus takes the left ~2/3; the rest stack on the right.
        let others = n - 1;
        let side_h = (1.0 - G * (others as f32 + 1.0)) / others as f32;
        let mut k = 0;
        (0..n)
            .map(|i| {
                if i == focus {
                    r(G, G, 0.66 - 1.5 * G, 1.0 - 2.0 * G)
                } else {
                    let y = G + k as f32 * (side_h + G);
                    k += 1;
                    r(0.66 + G * 0.5, y, 0.34 - 1.5 * G, side_h)
                }
            })
            .collect()
    };
    let row = |n: usize| -> Vec<Rect> {
        let w = (1.0 - G * (n as f32 + 1.0)) / n as f32;
        (0..n).map(|i| r(G + i as f32 * (w + G), 0.12, w, 0.76)).collect()
    };
    let grid = |n: usize| -> Vec<Rect> {
        let cols = if n <= 1 { 1 } else { 2 };
        let rows = n.div_ceil(cols);
        let w = (1.0 - G * (cols as f32 + 1.0)) / cols as f32;
        let h = (1.0 - G * (rows as f32 + 1.0)) / rows as f32;
        (0..n).map(|i| r(G + (i % cols) as f32 * (w + G), G + (i / cols) as f32 * (h + G), w, h)).collect()
    };
    match (layout, n) {
        (_, 0) => vec![],
        (_, 1) => vec![r(0.0, 0.0, 1.0, 1.0)],
        (Layout::Compare, _) | (Layout::Auto, 2) => row(n),
        (Layout::Hero, _) | (Layout::Auto, 3) => hero(n, focus),
        (Layout::Grid, _) | (Layout::Auto, _) => grid(n),
    }
}

/// True if the speech contains words that suggest re-arranging or annotating the board — used to
/// decide when an in-progress phrase is worth an agent call.
pub fn has_layout_cue(text: &str) -> bool {
    let t = text.to_lowercase();
    [
        "compare", "versus", " vs ", "side by side", "next to each other", "both of these", "focus on", "zoom in",
        "this one", "closer look", "all of these", "all together", "altogether", "notice", "see how", "look at the",
        "pay attention", "connects to", "leads to", "compared to", "just like", "moving on", "let's move on",
        "next topic", "new section", "set that aside",
    ]
    .iter()
    .any(|c| t.contains(c))
}

/// Offline agent (no API key): cue rules over the newest speech → ops. Transparent and cheap.
pub fn rule_ops(curr: &str, scene: &Scene) -> Vec<Op> {
    let t = curr.to_lowercase().replace('’', "'");
    let has = |cues: &[&str]| cues.iter().any(|c| t.contains(c));
    let n = scene.elements.len();
    let focused = scene.elements.iter().find(|e| e.focus).map(|e| e.id.clone());
    let mut ops = vec![];
    if has(&["let's move on", "moving on", "next topic", "new section", "set that aside", "start fresh"]) && n > 0 {
        return vec![Op::ClearBoard];
    }
    if has(&["compare", "versus", " vs ", "side by side", "next to each other", "both of these"]) && n >= 2 && scene.layout != Layout::Compare {
        ops.push(Op::Arrange { layout: Layout::Compare });
    } else if has(&["focus on", "zoom in", "this one", "take a closer look", "closer look"]) && n >= 2 && scene.layout != Layout::Hero {
        ops.push(Op::Arrange { layout: Layout::Hero });
    } else if has(&["all of these", "all together", "altogether", "the whole set", "everything we've seen"]) && n >= 3 {
        ops.push(Op::Arrange { layout: Layout::Grid });
    }
    if has(&["notice", "see how", "look at the", "pay attention to", "important"]) {
        if let Some(id) = focused.clone() {
            if !scene.annotations.iter().any(|a| a.targets == vec![id.clone()] && a.kind == AnnotationKind::Highlight) {
                ops.push(Op::Annotate { kind: AnnotationKind::Highlight, targets: vec![id], label: None });
            }
        }
    }
    if has(&["connects to", "leads to", "compared to", "relates to", "just like"]) && n >= 2 {
        let a = scene.elements[n - 2].id.clone();
        let b = scene.elements[n - 1].id.clone();
        ops.push(Op::Annotate { kind: AnnotationKind::Arrow, targets: vec![a, b], label: None });
    }
    ops
}

#[cfg(test)]
mod tests {
    use super::*;

    fn add(c: &mut Canvas, id: &str) -> Scene {
        c.render(id, id, &format!("img://localhost/{id}"), 1)
    }

    #[test]
    fn board_builds_up_and_evicts_oldest() {
        let mut c = Canvas::new();
        let s = add(&mut c, "eagle");
        assert_eq!(s.elements.len(), 1);
        assert_eq!(s.elements[0].rect, r(0.0, 0.0, 1.0, 1.0));
        add(&mut c, "owl");
        let s = add(&mut c, "parrot");
        assert_eq!(s.elements.len(), 3);
        assert!(s.elements.last().unwrap().focus);
        add(&mut c, "penguin");
        let s = add(&mut c, "zebra");
        assert_eq!(s.elements.len(), 4);
        assert!(!s.elements.iter().any(|e| e.image_id == "eagle"), "oldest evicted");
        assert_eq!(s.version, 5);
    }

    #[test]
    fn auto_layouts_are_inside_the_screen_and_do_not_overlap() {
        for n in 1..=4 {
            for layout in [Layout::Auto, Layout::Hero, Layout::Compare, Layout::Grid] {
                let rs = layout_rects(layout, n, n - 1);
                assert_eq!(rs.len(), n);
                for a in &rs {
                    assert!(a.x >= 0.0 && a.y >= 0.0 && a.x + a.w <= 1.0 + 1e-4 && a.y + a.h <= 1.0 + 1e-4, "{layout:?} {n} {a:?}");
                }
                for i in 0..n {
                    for j in i + 1..n {
                        let (a, b) = (rs[i], rs[j]);
                        let overlap = a.x < b.x + b.w - 1e-4 && b.x < a.x + a.w - 1e-4 && a.y < b.y + b.h - 1e-4 && b.y < a.y + a.h - 1e-4;
                        assert!(!overlap, "{layout:?} n={n} {a:?} {b:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn update_replaces_focused_in_place() {
        let mut c = Canvas::new();
        add(&mut c, "red-rose");
        let before = c.scene().elements[0].clone();
        let s = c.update("white-rose", "white rose", "img://localhost/white-rose", 2);
        assert_eq!(s.elements.len(), 1);
        assert_eq!(s.elements[0].id, before.id);
        assert_eq!(s.elements[0].image_id, "white-rose");
    }

    #[test]
    fn rerender_of_existing_image_refocuses() {
        let mut c = Canvas::new();
        add(&mut c, "eagle");
        add(&mut c, "owl");
        let s = add(&mut c, "eagle");
        assert_eq!(s.elements.len(), 2);
        assert!(s.elements.iter().find(|e| e.image_id == "eagle").unwrap().focus);
    }

    #[test]
    fn agent_ops_are_validated_and_version_checked() {
        let mut c = Canvas::new();
        add(&mut c, "eagle");
        let s = add(&mut c, "owl");
        let ids: Vec<String> = s.elements.iter().map(|e| e.id.clone()).collect();
        // stale version → dropped
        assert!(c.apply(s.version - 1, &[Op::Arrange { layout: Layout::Compare }], 3).is_none());
        // unknown id ignored; valid ops applied
        let out = c
            .apply(
                s.version,
                &[
                    Op::Focus { id: "nope".into() },
                    Op::Annotate { kind: AnnotationKind::Highlight, targets: vec![ids[0].clone(), "nope".into()], label: Some("x".repeat(80)) },
                ],
                3,
            )
            .unwrap();
        assert_eq!(out.annotations.len(), 1);
        assert_eq!(out.annotations[0].targets, vec![ids[0].clone()]);
        assert_eq!(out.annotations[0].label.as_ref().unwrap().len(), 40);
        assert!(out.reason.starts_with("agent:"));
        // removing an annotated element removes its annotation
        let out = c.apply(out.version, &[Op::Remove { id: ids[0].clone() }], 4).unwrap();
        assert_eq!(out.elements.len(), 1);
        assert!(out.annotations.is_empty());
        assert!(out.elements[0].focus);
    }

    #[test]
    fn clear_empties_board() {
        let mut c = Canvas::new();
        add(&mut c, "eagle");
        let s = c.clear(9);
        assert!(s.elements.is_empty() && s.annotations.is_empty());
    }

    #[test]
    fn rules_map_cues_to_ops() {
        let mut c = Canvas::new();
        add(&mut c, "eagle");
        let s = add(&mut c, "owl");
        assert_eq!(rule_ops("let's compare these two birds", &s), vec![Op::Arrange { layout: Layout::Compare }]);
        assert!(matches!(rule_ops("notice the beak", &s)[0], Op::Annotate { kind: AnnotationKind::Highlight, .. }));
        assert_eq!(rule_ops("ok, moving on", &s), vec![Op::ClearBoard]);
        assert!(rule_ops("they live in forests", &s).is_empty());
    }
}
