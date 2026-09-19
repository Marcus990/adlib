//! Canvas mode (CANVAS.md): an evolving board of ≤ 4 elements — library images, live diagrams
//! (flow / cycle / hub / timeline) and charts built from spoken numbers — plus ≤ 3 annotations.
//! Pure and deterministic. The fast path (stage renders) and the agent (tool calls) both mutate the
//! board through [`Canvas`]; every mutation bumps `version` so stale agent answers can be dropped.

use serde::{Deserialize, Serialize};

pub const MAX_ELEMENTS: usize = 4;
pub const MAX_ANNOTATIONS: usize = 3;
pub const MAX_NODES: usize = 8;
pub const MAX_POINTS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ElementKind {
    #[default]
    Image,
    Diagram,
    Chart,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Element {
    pub id: String,
    #[serde(default)]
    pub kind: ElementKind,
    /// Library image id (empty for diagrams and charts).
    pub image_id: String,
    /// Image caption, or the graphic's title.
    pub caption: String,
    pub url: String,
    pub rect: Rect,
    pub z: u32,
    pub focus: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagram: Option<Diagram>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chart: Option<Chart>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagramLayout {
    /// Steps / cause → effect, left to right (layered if it branches).
    Flow,
    /// Steps that repeat (last → first).
    Cycle,
    /// A central idea and its parts.
    Hub,
    /// Dated events along an axis.
    Timeline,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Secondary text: a year on a timeline, a short detail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    pub from: String,
    pub to: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Diagram {
    pub layout: DiagramLayout,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    /// Edges were implied by order (flow/cycle/timeline chain, hub spokes); extending continues them.
    #[serde(default)]
    pub auto_edges: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChartKind {
    Bar,
    Line,
    Pie,
    /// One big number (or "from → to" with the change when there are two points).
    Stat,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub label: String,
    pub value: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Chart {
    pub kind: ChartKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    pub points: Vec<Point>,
}

/// A node as the agent describes it (ids are assigned by the canvas).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeSpec {
    pub label: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
}

/// An edge as the agent describes it: endpoints by node label (or node id).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EdgeSpec {
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub label: Option<String>,
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
    DrawDiagram { layout: DiagramLayout, title: Option<String>, nodes: Vec<NodeSpec>, edges: Vec<EdgeSpec> },
    ExtendDiagram { id: String, nodes: Vec<NodeSpec>, edges: Vec<EdgeSpec> },
    DrawChart { kind: ChartKind, title: Option<String>, unit: Option<String>, points: Vec<Point> },
    UpdateChart { id: String, kind: Option<ChartKind>, title: Option<String>, points: Vec<Point> },
}

impl Op {
    /// Content-adding ops don't depend on the board's arrangement, so they still apply when the fast
    /// path changed the board while the agent was thinking (a 2 s chart must not be thrown away).
    pub fn is_additive(&self) -> bool {
        matches!(self, Op::DrawDiagram { .. } | Op::ExtendDiagram { .. } | Op::DrawChart { .. } | Op::UpdateChart { .. })
    }
}

fn short(s: &str, n: usize) -> String {
    s.trim().chars().take(n).collect()
}

fn same(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

fn clean_nodes(specs: &[NodeSpec], existing: &[Node]) -> Vec<(String, Option<String>, Option<String>)> {
    let mut out: Vec<(String, Option<String>, Option<String>)> = vec![];
    for n in specs {
        let label = short(&n.label, 32);
        if label.is_empty() || existing.iter().any(|e| same(&e.label, &label)) || out.iter().any(|o| same(&o.0, &label)) {
            continue;
        }
        let icon = n.icon.as_deref().map(|i| short(i, 4)).filter(|i| !i.is_empty());
        let note = n.note.as_deref().map(|t| short(t, 24)).filter(|t| !t.is_empty());
        out.push((label, icon, note));
    }
    out
}

fn resolve(nodes: &[Node], key: &str) -> Option<String> {
    nodes.iter().find(|n| n.id == key.trim() || same(&n.label, key)).map(|n| n.id.clone())
}

/// A "stat" is one number or a before → after; three or more values read better as bars.
fn promote(kind: ChartKind, n: usize) -> ChartKind {
    if kind == ChartKind::Stat && n >= 3 { ChartKind::Bar } else { kind }
}

fn clean_points(points: &[Point]) -> Vec<Point> {
    let mut out: Vec<Point> = vec![];
    for p in points {
        let label = short(&p.label, 24);
        if !p.value.is_finite() || out.iter().any(|o| same(&o.label, &label)) {
            continue;
        }
        out.push(Point { label, value: p.value });
    }
    out.truncate(MAX_POINTS);
    out
}

impl Diagram {
    fn add_nodes(&mut self, specs: &[NodeSpec]) -> Vec<String> {
        let mut added = vec![];
        for (label, icon, note) in clean_nodes(specs, &self.nodes) {
            if self.nodes.len() >= MAX_NODES {
                break;
            }
            let id = format!("n{}", self.nodes.len() + 1);
            self.nodes.push(Node { id: id.clone(), label, icon, note });
            added.push(id);
        }
        added
    }

    fn add_edges(&mut self, specs: &[EdgeSpec]) -> usize {
        let mut n = 0;
        for e in specs {
            let (Some(from), Some(to)) = (resolve(&self.nodes, &e.from), resolve(&self.nodes, &e.to)) else { continue };
            if from == to || self.edges.iter().any(|x| x.from == from && x.to == to) {
                continue;
            }
            self.edges.push(Edge { from, to, label: e.label.as_deref().map(|l| short(l, 24)).filter(|l| !l.is_empty()) });
            n += 1;
        }
        n
    }

    /// Chain / spokes implied by the layout, for nodes `from_idx..`.
    fn auto_link(&mut self, from_idx: usize) {
        let ids: Vec<String> = self.nodes.iter().map(|n| n.id.clone()).collect();
        match self.layout {
            DiagramLayout::Hub => {
                for id in ids.iter().skip(from_idx.max(1)) {
                    self.edges.push(Edge { from: ids[0].clone(), to: id.clone(), label: None });
                }
            }
            _ => {
                // Drop the closing edge of a cycle before extending the chain, then re-close it.
                if self.layout == DiagramLayout::Cycle {
                    if let (Some(first), Some(last)) = (ids.first(), ids.get(from_idx.saturating_sub(1))) {
                        self.edges.retain(|e| !(&e.from == last && &e.to == first));
                    }
                }
                for i in from_idx.max(1)..ids.len() {
                    self.edges.push(Edge { from: ids[i - 1].clone(), to: ids[i].clone(), label: None });
                }
                if self.layout == DiagramLayout::Cycle && ids.len() >= 3 {
                    self.edges.push(Edge { from: ids[ids.len() - 1].clone(), to: ids[0].clone(), label: None });
                }
            }
        }
    }
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
        self.push_element(ElementKind::Image, image_id, caption, url, None, None);
        self.bump(format!("fast:render {image_id}"), chunk_id)
    }

    /// Add an element, focus it, auto layout; evict the oldest past the cap. Returns its id.
    fn push_element(&mut self, kind: ElementKind, image_id: &str, caption: &str, url: &str, diagram: Option<Diagram>, chart: Option<Chart>) -> String {
        let id = self.new_id("e");
        let z = self.scene.elements.iter().map(|e| e.z).max().unwrap_or(0) + 1;
        for e in self.scene.elements.iter_mut() {
            e.focus = false;
        }
        self.scene.elements.push(Element {
            id: id.clone(),
            kind,
            image_id: image_id.into(),
            caption: caption.into(),
            url: url.into(),
            rect: Rect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 },
            z,
            focus: true,
            diagram,
            chart,
        });
        while self.scene.elements.len() > MAX_ELEMENTS {
            let old = self.scene.elements.remove(0);
            self.scene.annotations.retain(|a| !a.targets.contains(&old.id));
        }
        self.scene.layout = Layout::Auto;
        id
    }

    fn focus_only(&mut self, id: &str) {
        for e in self.scene.elements.iter_mut() {
            e.focus = e.id == id;
        }
    }

    /// Refinement: replace the focused image in place (keeps its position and id).
    pub fn update(&mut self, image_id: &str, caption: &str, url: &str, chunk_id: u64) -> Scene {
        let images = || self.scene.elements.iter().filter(|e| e.kind == ElementKind::Image);
        let target = images().find(|e| e.focus).or_else(|| images().last()).map(|e| e.id.clone());
        match self.scene.elements.iter_mut().find(|e| Some(&e.id) == target.as_ref()) {
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
    /// Content-adding ops (diagrams, charts) apply even when stale; see [`Op::is_additive`].
    pub fn apply(&mut self, expected_version: u64, ops: &[Op], chunk_id: u64) -> Option<Scene> {
        let stale = self.scene.version != expected_version;
        let mut applied = vec![];
        for op in ops {
            if stale && !op.is_additive() {
                continue;
            }
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
            Op::DrawDiagram { layout, title, nodes, edges } => {
                let mut d = Diagram { layout: *layout, title: title.as_deref().map(|t| short(t, 48)).filter(|t| !t.is_empty()), nodes: vec![], edges: vec![], auto_edges: false };
                d.add_nodes(nodes);
                if d.nodes.len() < 2 {
                    return false;
                }
                if d.add_edges(edges) == 0 {
                    d.auto_edges = true;
                    d.auto_link(0);
                }
                // Models often redraw a diagram instead of extending it: if one on the board shares
                // most of these nodes, replace it in place (keeps its tile and the animation calm).
                let labels: Vec<&str> = d.nodes.iter().map(|n| n.label.as_str()).collect();
                let twin = self.scene.elements.iter().position(|e| {
                    e.diagram.as_ref().is_some_and(|o| {
                        let shared = o.nodes.iter().filter(|n| labels.iter().any(|l| same(l, &n.label))).count();
                        shared * 2 >= o.nodes.len().max(2)
                    })
                });
                let caption = d.title.clone().unwrap_or_else(|| labels.join(" → "));
                match twin {
                    Some(i) => {
                        let id = self.scene.elements[i].id.clone();
                        self.scene.elements[i].diagram = Some(d);
                        self.scene.elements[i].caption = caption;
                        self.focus_only(&id);
                    }
                    None => {
                        self.push_element(ElementKind::Diagram, "", &caption, "", Some(d), None);
                    }
                }
                true
            }
            Op::ExtendDiagram { id, nodes, edges } => {
                let Some(e) = self.scene.elements.iter_mut().find(|e| &e.id == id && e.diagram.is_some()) else { return false };
                let d = e.diagram.as_mut().unwrap();
                let before = d.nodes.len();
                let added = d.add_nodes(nodes);
                let linked = d.add_edges(edges);
                if !added.is_empty() && linked == 0 && d.auto_edges {
                    d.auto_link(before);
                }
                let changed = !added.is_empty() || linked > 0;
                if changed {
                    let id = id.clone();
                    self.focus_only(&id);
                }
                changed
            }
            Op::DrawChart { kind, title, unit, points } => {
                let points = clean_points(points);
                if points.is_empty() {
                    return false;
                }
                let title = title.as_deref().map(|t| short(t, 48)).filter(|t| !t.is_empty());
                let unit = unit.as_deref().map(|u| short(u, 16)).filter(|u| !u.is_empty());
                let kind = promote(*kind, points.len());
                // Same title as a chart on the board → a corrected redraw: replace its data in place.
                if let Some(t) = &title {
                    if let Some(e) = self.scene.elements.iter_mut().find(|e| e.chart.as_ref().is_some_and(|c| c.title.as_deref().is_some_and(|x| same(x, t)))) {
                        let new = Chart { kind, title: title.clone(), unit: unit.or_else(|| e.chart.as_ref().unwrap().unit.clone()), points };
                        let changed = e.chart.as_ref() != Some(&new);
                        e.chart = Some(new);
                        let id = e.id.clone();
                        self.focus_only(&id);
                        return changed;
                    }
                }
                let caption = title.clone().unwrap_or_else(|| "chart".into());
                self.push_element(ElementKind::Chart, "", &caption, "", None, Some(Chart { kind, title, unit, points }));
                true
            }
            Op::UpdateChart { id, kind, title, points } => {
                let Some(e) = self.scene.elements.iter_mut().find(|e| &e.id == id && e.chart.is_some()) else { return false };
                if let Some(t) = title.as_deref().map(|t| short(t, 48)).filter(|t| !t.is_empty()) {
                    e.caption = t.clone();
                    e.chart.as_mut().unwrap().title = Some(t);
                }
                let c = e.chart.as_mut().unwrap();
                let before = c.clone();
                if let Some(k) = kind {
                    c.kind = *k;
                }
                // The full data set, not a patch: speech arrives in fragments ("60% are stoned…" →
                // "…students, 30% teachers, 10% parents"), and the newest, most complete call wins.
                let points = clean_points(points);
                if !points.is_empty() {
                    c.points = points;
                }
                c.kind = promote(c.kind, c.points.len());
                let changed = *c != before || title.is_some();
                if changed {
                    let id = id.clone();
                    self.focus_only(&id);
                }
                changed
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
        Op::DrawDiagram { layout, nodes, .. } => format!("draw_diagram({layout:?}, {} nodes)", nodes.len()).to_lowercase(),
        Op::ExtendDiagram { id, nodes, .. } => format!("extend_diagram({id}, +{})", nodes.len()),
        Op::DrawChart { kind, points, .. } => format!("draw_chart({kind:?}, {} points)", points.len()).to_lowercase(),
        Op::UpdateChart { id, points, .. } => format!("update_chart({id}, {} points)", points.len()),
    }
}

/// The presenter explicitly closes a section — the only time the agent may clear the board.
pub fn has_section_cue(text: &str) -> bool {
    let t = text.to_lowercase().replace('’', "'");
    [
        "move on", "moving on", "next topic", "new section", "set that aside", "start fresh", "switch gears",
        "switching gears", "next up", "clean slate", "clear the screen", "change of topic", "different topic",
    ]
    .iter()
    .any(|c| t.contains(c))
}

/// True if the speech carries numbers or structure (steps, cause → effect, cycles, change over time)
/// that a diagram or chart could show — used to decide when to ask the agent.
pub fn has_graphic_cue(text: &str) -> bool {
    let t = text.to_lowercase();
    if t.chars().any(|c| c.is_ascii_digit()) {
        return true;
    }
    [
        "percent", "hundred", "thousand", "million", "billion", "half of", "a third", "a quarter", "doubled", "tripled",
        "twice as", "grew", "growth", "increased", "decreased", "dropped", "went up", "went down", "first,", "first we",
        "first you", "first the", "then we", "then you", "then the", "then it", "after that", "next,", "finally", "step",
        "stages", "phase", "process", "pipeline", "workflow", "cycle", "loop", "leads to", "results in", "feeds into",
        "which means", "because of", "timeline", "over the years", "made up of", "consists of", "breaks down", "parts:",
    ]
    .iter()
    .any(|c| t.contains(c))
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

    fn ns(labels: &[&str]) -> Vec<NodeSpec> {
        labels.iter().map(|l| NodeSpec { label: l.to_string(), icon: None, note: None }).collect()
    }
    fn pts(v: &[(&str, f64)]) -> Vec<Point> {
        v.iter().map(|(l, x)| Point { label: l.to_string(), value: *x }).collect()
    }

    #[test]
    fn diagrams_draw_chain_and_extend() {
        let mut c = Canvas::new();
        let v = c.scene().version;
        let s = c.apply(v, &[Op::DrawDiagram { layout: DiagramLayout::Flow, title: Some("How it works".into()), nodes: ns(&["Speak", "Transcribe", "speak"]), edges: vec![] }], 1).unwrap();
        let e = &s.elements[0];
        assert_eq!(e.kind, ElementKind::Diagram);
        let d = e.diagram.as_ref().unwrap();
        assert_eq!(d.nodes.len(), 2, "duplicate label dropped");
        assert_eq!(d.edges, vec![Edge { from: "n1".into(), to: "n2".into(), label: None }], "chain implied by order");
        let id = e.id.clone();
        let s = c.apply(s.version, &[Op::ExtendDiagram { id: id.clone(), nodes: ns(&["Decide", "Show"]), edges: vec![] }], 2).unwrap();
        let d = s.elements[0].diagram.as_ref().unwrap();
        assert_eq!(d.nodes.len(), 4);
        assert_eq!(d.edges.len(), 3);
        assert_eq!(d.edges[2].to, "n4");
        // a redraw with mostly the same nodes replaces in place instead of adding a tile
        let s = c.apply(s.version, &[Op::DrawDiagram { layout: DiagramLayout::Flow, title: None, nodes: ns(&["Speak", "Transcribe", "Decide", "Show", "Repeat"]), edges: vec![] }], 3).unwrap();
        assert_eq!(s.elements.len(), 1);
        assert_eq!(s.elements[0].id, id);
        assert_eq!(s.elements[0].diagram.as_ref().unwrap().nodes.len(), 5);
    }

    #[test]
    fn cycle_closes_and_explicit_edges_resolve_by_label() {
        let mut c = Canvas::new();
        let s = c.apply(0, &[Op::DrawDiagram { layout: DiagramLayout::Cycle, title: None, nodes: ns(&["Listen", "Decide", "Show"]), edges: vec![] }], 1).unwrap();
        let d = s.elements[0].diagram.as_ref().unwrap();
        assert!(d.edges.iter().any(|e| e.from == "n3" && e.to == "n1"), "cycle closes");
        let id = s.elements[0].id.clone();
        let s = c.apply(s.version, &[Op::ExtendDiagram { id, nodes: ns(&["Learn"]), edges: vec![] }], 2).unwrap();
        let d = s.elements[0].diagram.as_ref().unwrap();
        assert!(d.edges.iter().any(|e| e.from == "n4" && e.to == "n1") && !d.edges.iter().any(|e| e.from == "n3" && e.to == "n1"), "{:?}", d.edges);
        let s = c.apply(s.version, &[Op::DrawDiagram { layout: DiagramLayout::Flow, title: None, nodes: ns(&["Rain", "Flood", "Drought"]),
            edges: vec![EdgeSpec { from: "rain".into(), to: "Flood".into(), label: Some("too much".into()) }, EdgeSpec { from: "x".into(), to: "Flood".into(), label: None }] }], 3).unwrap();
        let d = s.elements[1].diagram.as_ref().unwrap();
        assert_eq!(d.edges.len(), 1);
        assert!(!d.auto_edges);
        assert!(c.apply(s.version, &[Op::DrawDiagram { layout: DiagramLayout::Hub, title: None, nodes: ns(&["alone"]), edges: vec![] }], 4).is_none(), "one node is not a diagram");
    }

    #[test]
    fn charts_draw_merge_and_survive_a_stale_board() {
        let mut c = Canvas::new();
        let s = c.apply(0, &[Op::DrawChart { kind: ChartKind::Bar, title: Some("Users".into()), unit: None, points: pts(&[("2024", 2000.0), ("2025", f64::NAN)]) }], 1).unwrap();
        assert_eq!(s.elements[0].chart.as_ref().unwrap().points.len(), 1, "non-finite dropped");
        let id = s.elements[0].id.clone();
        // the fast path adds an image meanwhile → the agent's scene is stale, but a chart still lands
        let stale = s.version;
        add(&mut c, "eagle");
        let s = c.apply(stale, &[Op::UpdateChart { id: id.clone(), kind: None, title: None, points: pts(&[("2024", 2500.0), ("2025", 15000.0)]) }, Op::Arrange { layout: Layout::Grid }], 2).unwrap();
        let ch = s.elements.iter().find(|e| e.id == id).unwrap().chart.as_ref().unwrap();
        assert_eq!(ch.points, pts(&[("2024", 2500.0), ("2025", 15000.0)]), "update replaces the data set");
        assert!(c.apply(s.version, &[Op::UpdateChart { id: id.clone(), kind: None, title: None, points: vec![] }], 2).is_none(), "empty update ignored");
        assert_eq!(s.layout, Layout::Auto, "non-additive op skipped on a stale board");
        // same title → a corrected redraw replaces that chart's data instead of adding a second chart
        let s = c.apply(s.version, &[Op::DrawChart { kind: ChartKind::Line, title: Some("users".into()), unit: None, points: pts(&[("2024", 2000.0), ("2025", 15000.0), ("2026", 40000.0)]) }], 3).unwrap();
        assert_eq!(s.elements.len(), 2);
        let ch = s.elements.iter().find(|e| e.id == id).unwrap().chart.as_ref().unwrap();
        assert_eq!((ch.kind, ch.points.len()), (ChartKind::Line, 3));
        // a stat that grows to 3 values becomes bars
        let s = c.apply(s.version, &[Op::DrawChart { kind: ChartKind::Stat, title: None, unit: None, points: pts(&[("a", 1.0), ("b", 2.0), ("c", 3.0)]) }], 5).unwrap();
        assert_eq!(s.elements.last().unwrap().chart.as_ref().unwrap().kind, ChartKind::Bar);
        // image refinement still targets the image, not the focused chart
        let s = c.update("owl", "owl", "u", 4);
        assert!(s.elements.iter().any(|e| e.image_id == "owl") && !s.elements.iter().any(|e| e.image_id == "eagle"));
    }

    #[test]
    fn graphic_cues() {
        assert!(has_graphic_cue("we grew to 15,000 users"));
        assert!(has_graphic_cue("about sixty percent are students"));
        assert!(has_graphic_cue("First, we record. Then the speech is transcribed."));
        assert!(!has_graphic_cue("penguins are incredible swimmers"));
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
