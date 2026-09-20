//! Canvas mode (CANVAS.md): an evolving board of ≤ 4 elements — library images, live diagrams
//! (flow / cycle / hub / timeline) and charts built from spoken numbers — plus ≤ 3 annotations.
//! Pure and deterministic. The fast path (stage renders) and the agent (tool calls) both mutate the
//! board through [`Canvas`]; every mutation bumps `version` so stale agent answers can be dropped.

use serde::{Deserialize, Serialize};

pub const MAX_ELEMENTS: usize = 4;
pub const MAX_ANNOTATIONS: usize = 3;
pub const MAX_NODES: usize = 10;
pub const MAX_POINTS: usize = 8;
pub const MAX_TEXT_BLOCKS: usize = 8;

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
    Text,
    /// A company logo, icon or flag from the symbol library, or a plain name card when the library has none
    /// (`image_id` empty, `caption` = the name).
    Logo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Element {
    pub id: String,
    #[serde(default)]
    pub kind: ElementKind,
    /// Library image id, or the symbol's id for a logo tile (empty for diagrams, charts and name cards).
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<TextCard>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextBlockKind {
    Heading,
    Paragraph,
    Bullet,
}

/// One semantic unit in a text tile. IDs are assigned by the canvas so Luna can patch a single line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextBlock {
    pub id: String,
    pub kind: TextBlockKind,
    pub text: String,
    #[serde(default)]
    pub level: u8,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub emphasis: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextCard {
    pub blocks: Vec<TextBlock>,
}

/// A text block as the agent describes it; the canvas owns stable block IDs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextBlockSpec {
    pub kind: TextBlockKind,
    pub text: String,
    #[serde(default)]
    pub level: u8,
    #[serde(default)]
    pub emphasis: Vec<String>,
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
    /// An image url (a logo or icon from the symbol library), or a short emoji.
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
    /// An icon or logo drawn with the point: an image url once the pipeline has resolved it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// The brand the model named for this point (a hint the pipeline resolves into `icon`; never drawn as is).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo: Option<String>,
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
    /// What the model asked for: a generic icon name ("database"); the pipeline replaces it with the image url.
    #[serde(default)]
    pub icon: Option<String>,
    /// The product or company the node is ("Postgres", "Kafka"): looked up in the logo library by the pipeline.
    #[serde(default)]
    pub logo: Option<String>,
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

/// One validated agent operation (tool call). Ops address things by id (tiles `e1`, diagram nodes `n1`) and
/// chart points by label. Destructive ops carry the `quote` the model says asked for them; the agent layer
/// checks it against the newest speech.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    Focus { id: String },
    Remove { id: String, #[serde(default)] quote: Option<String> },
    Arrange { layout: Layout },
    Annotate { kind: AnnotationKind, targets: Vec<String>, label: Option<String> },
    ClearAnnotations,
    ClearBoard { #[serde(default)] quote: Option<String> },
    DrawDiagram { layout: DiagramLayout, title: Option<String>, nodes: Vec<NodeSpec>, edges: Vec<EdgeSpec> },
    AddNodes { id: String, nodes: Vec<NodeSpec>, edges: Vec<EdgeSpec> },
    UpdateNode { id: String, node: String, label: Option<String>, note: Option<String> },
    RemoveNode { id: String, node: String, #[serde(default)] quote: Option<String> },
    AddEdge { id: String, from: String, to: String, label: Option<String> },
    RemoveEdge { id: String, from: String, to: String },
    DrawChart { kind: ChartKind, title: Option<String>, unit: Option<String>, points: Vec<Point> },
    /// Replace a chart's whole data set. Not offered to the model (it patches with the point ops); kept for setup and tests.
    UpdateChart { id: String, kind: Option<ChartKind>, title: Option<String>, points: Vec<Point> },
    SetPoint { id: String, label: String, value: f64 },
    AddPoint { id: String, label: String, value: f64, #[serde(default)] icon: Option<String>, #[serde(default)] logo: Option<String> },
    RemovePoint { id: String, label: String, #[serde(default)] quote: Option<String> },
    SetChart { id: String, kind: Option<ChartKind>, title: Option<String>, unit: Option<String> },
    DrawText { blocks: Vec<TextBlockSpec> },
    AddTextBlocks { id: String, blocks: Vec<TextBlockSpec> },
    UpdateTextBlock { id: String, block: String, text: Option<String>, emphasis: Option<Vec<String>>, level: Option<u8> },
    RemoveTextBlock { id: String, block: String, #[serde(default)] quote: Option<String> },
    /// A photo request. The canvas does not place photos itself: the pipeline searches the library
    /// (or draws one) and renders the result.
    ShowPhoto { subject: String, replace: bool },
    /// A company / product logo request, looked up by name (not by embedding). The pipeline resolves it.
    ShowLogo { name: String, #[serde(default)] replace: bool },
    /// A generic icon or flag request ("teamwork", "database", "flag of Canada"): the concept plus synonyms the
    /// model offered, looked up by name and tags. The pipeline resolves it.
    ShowIcon { concept: String, #[serde(default)] alternatives: Vec<String>, #[serde(default)] replace: bool },
    /// The model chose to do nothing, and why (logged).
    NoAction { reason: String },
}

impl Op {
    /// Changes what a chart or diagram *shows* (as opposed to layout, focus, photos or removals).
    pub fn is_graphic(&self) -> bool {
        matches!(
            self,
            Op::DrawDiagram { .. } | Op::AddNodes { .. } | Op::UpdateNode { .. } | Op::RemoveNode { .. } | Op::AddEdge { .. } | Op::RemoveEdge { .. }
                | Op::DrawChart { .. } | Op::UpdateChart { .. } | Op::SetPoint { .. } | Op::AddPoint { .. } | Op::RemovePoint { .. } | Op::SetChart { .. }
                | Op::DrawText { .. } | Op::AddTextBlocks { .. } | Op::UpdateTextBlock { .. } | Op::RemoveTextBlock { .. }
        )
    }
    /// The words the model quoted as its authority for a destructive op, if this op needs one.
    pub fn quote(&self) -> Option<Option<&str>> {
        match self {
            Op::Remove { quote, .. } | Op::ClearBoard { quote } | Op::RemoveNode { quote, .. } | Op::RemovePoint { quote, .. } | Op::RemoveTextBlock { quote, .. } => Some(quote.as_deref()),
            _ => None,
        }
    }
}

fn short(s: &str, n: usize) -> String {
    s.trim().chars().take(n).collect()
}

fn same(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

/// Index of the point a spoken label means: exact (case-insensitive), then letters-and-digits only, then one
/// label a prefix of the other ("Mar" ↔ "March"), then one contained in the other. `None` if nothing is close.
fn find_point(points: &[Point], label: &str) -> Option<usize> {
    let norm = |s: &str| s.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase();
    let want = norm(label);
    if want.is_empty() {
        return None;
    }
    let pass = |f: &dyn Fn(&str, &str) -> bool| points.iter().position(|p| f(&norm(&p.label), &want));
    pass(&|have, want| have == want)
        .or_else(|| pass(&|have, want| want.len() >= 3 && have.len() >= 3 && (have.starts_with(want) || want.starts_with(have))))
        .or_else(|| pass(&|have, want| want.len() >= 3 && have.len() >= 3 && (have.contains(want) || want.contains(have))))
}

fn clean_nodes(specs: &[NodeSpec], existing: &[Node]) -> Vec<(String, Option<String>, Option<String>)> {
    let mut out: Vec<(String, Option<String>, Option<String>)> = vec![];
    for n in specs {
        let label = short(&n.label, 32);
        if label.is_empty() || existing.iter().any(|e| same(&e.label, &label)) || out.iter().any(|o| same(&o.0, &label)) {
            continue;
        }
        let icon = n.icon.as_deref().map(|i| short(i, if i.contains("://") { 200 } else { 4 })).filter(|i| !i.is_empty());
        let note = n.note.as_deref().map(|t| short(t, 24)).filter(|t| !t.is_empty());
        out.push((label, icon, note));
    }
    out
}

fn resolve(nodes: &[Node], key: &str) -> Option<String> {
    nodes.iter().find(|n| n.id == key.trim() || same(&n.label, key)).map(|n| n.id.clone())
}

/// A "stat" is ONE headline number. As soon as a second value exists the presenter is comparing
/// quantities, and that reads as bars — a 2-point stat renders as two numbers joined by an arrow
/// (sketch.js:293), which is what "200 users last year, 300 the year before" drew on 09-19.
fn promote(kind: ChartKind, n: usize) -> ChartKind {
    if kind == ChartKind::Stat && n >= 2 { ChartKind::Bar } else { kind }
}

fn clean_points(points: &[Point]) -> Vec<Point> {
    let mut out: Vec<Point> = vec![];
    for p in points {
        let label = short(&p.label, 24);
        if !p.value.is_finite() || out.iter().any(|o| same(&o.label, &label)) {
            continue;
        }
        out.push(Point { label, value: p.value, icon: p.icon.clone().filter(|i| !i.is_empty()), logo: p.logo.clone().filter(|i| !i.is_empty()) });
    }
    out.truncate(MAX_POINTS);
    out
}

fn clean_text_spec(spec: &TextBlockSpec) -> Option<TextBlockSpec> {
    let limit = match spec.kind {
        TextBlockKind::Heading => 80,
        TextBlockKind::Paragraph => 220,
        TextBlockKind::Bullet => 140,
    };
    let text = short(&spec.text, limit);
    if text.is_empty() {
        return None;
    }
    let lower = text.to_lowercase();
    let mut emphasis: Vec<String> = vec![];
    for phrase in &spec.emphasis {
        let phrase = short(phrase, 48);
        if !phrase.is_empty() && lower.contains(&phrase.to_lowercase()) && !emphasis.iter().any(|p| same(p, &phrase)) {
            emphasis.push(phrase);
        }
        if emphasis.len() == 2 {
            break;
        }
    }
    if emphasis.is_empty() {
        return None;
    }
    let level = match spec.kind {
        TextBlockKind::Heading => spec.level.clamp(1, 2),
        TextBlockKind::Bullet => spec.level.min(1),
        TextBlockKind::Paragraph => 0,
    };
    Some(TextBlockSpec { kind: spec.kind, text, level, emphasis })
}

impl Diagram {
    fn add_nodes(&mut self, specs: &[NodeSpec]) -> Vec<String> {
        let mut added = vec![];
        for (label, icon, note) in clean_nodes(specs, &self.nodes) {
            if self.nodes.len() >= MAX_NODES {
                break;
            }
            let next = self.nodes.iter().filter_map(|n| n.id.strip_prefix('n').and_then(|d| d.parse::<usize>().ok())).max().unwrap_or(0) + 1;
            let id = format!("n{next}");
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
    next_block_id: u64,
    /// Why ops were refused or did nothing (missing target, no such point, chart full…). Drained by [`Canvas::take_notes`].
    notes: Vec<String>,
    /// Names of the ops the last `apply` changed something with.
    applied: Vec<String>,
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

    fn new_block_id(&mut self) -> String {
        self.next_block_id += 1;
        format!("b{}", self.next_block_id)
    }

    // ---- fast path (from stage renders) ----

    /// New image: add it, focus it, auto layout; evict the oldest past the cap.
    pub fn render(&mut self, image_id: &str, caption: &str, url: &str, chunk_id: u64) -> Scene {
        if let Some(e) = self.scene.elements.iter_mut().find(|e| e.image_id == image_id) {
            // Already on the board: just bring it into focus.
            let id = e.id.clone();
            return self.focus_inner(&id, format!("fast:refocus {image_id}"), chunk_id);
        }
        self.push_element(ElementKind::Image, image_id, caption, url, None, None, None);
        self.bump(format!("fast:render {image_id}"), chunk_id)
    }

    /// A logo / icon / flag tile (`asset_id` = the library id, `url` its image) or, with an empty `asset_id`, a plain
    /// name card. Asking for one that is already on the board only refocuses it.
    pub fn render_logo(&mut self, asset_id: &str, caption: &str, url: &str, chunk_id: u64) -> Scene {
        let same = |e: &Element| {
            e.kind == ElementKind::Logo && if asset_id.is_empty() { e.image_id.is_empty() && same(&e.caption, caption) } else { e.image_id == asset_id }
        };
        if let Some(id) = self.scene.elements.iter().find(|e| same(e)).map(|e| e.id.clone()) {
            return self.focus_inner(&id, format!("fast:refocus logo {caption}"), chunk_id);
        }
        self.push_element(ElementKind::Logo, asset_id, caption, url, None, None, None);
        self.bump(format!("fast:logo {caption}"), chunk_id)
    }

    /// The words turned out to mean a different symbol than the one just shown ("the flag…" → "…of Canada"): swap the focused
    /// logo / icon / name card in place (else the newest one), keeping its tile. With none on the board it adds one.
    pub fn replace_logo(&mut self, asset_id: &str, caption: &str, url: &str, chunk_id: u64) -> Scene {
        let target = self.scene.elements.iter().find(|e| e.kind == ElementKind::Logo && e.focus).or_else(|| self.scene.elements.iter().rev().find(|e| e.kind == ElementKind::Logo)).map(|e| e.id.clone());
        let Some(id) = target else { return self.render_logo(asset_id, caption, url, chunk_id) };
        if let Some(e) = self.scene.elements.iter_mut().find(|e| e.id == id) {
            e.image_id = asset_id.into();
            e.caption = caption.into();
            e.url = url.into();
        }
        self.focus_inner(&id, format!("fast:replace logo {caption}"), chunk_id)
    }

    /// Add an element, focus it, auto layout; evict the oldest past the cap. Returns its id.
    fn push_element(&mut self, kind: ElementKind, image_id: &str, caption: &str, url: &str, diagram: Option<Diagram>, chart: Option<Chart>, text: Option<TextCard>) -> String {
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
            text,
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

    /// Apply validated ops. Ops address tiles by id, so a board that changed while the agent was thinking
    /// (a photo landed, another op applied) does not make them stale: each op applies if its target still
    /// exists, and says why in [`Canvas::take_notes`] if not. `expected_version` is kept for callers that
    /// only know the version; a stale `clear_board` then clears everything, use [`Canvas::apply_seen`] to
    /// clear only what the agent saw. Returns the new scene if anything changed.
    pub fn apply(&mut self, _expected_version: u64, ops: &[Op], chunk_id: u64) -> Option<Scene> {
        self.apply_inner(None, ops, chunk_id)
    }

    /// Like [`Canvas::apply`], for ops decided against `seen`: a `clear_board` removes only the tiles that were
    /// on the board then, so a photo that arrived while the agent was thinking survives the clear.
    pub fn apply_seen(&mut self, seen: &Scene, ops: &[Op], chunk_id: u64) -> Option<Scene> {
        self.apply_inner(Some(seen), ops, chunk_id)
    }

    /// Reasons ops were refused since the last call.
    pub fn take_notes(&mut self) -> Vec<String> {
        std::mem::take(&mut self.notes)
    }

    /// Names of the ops the last `apply` / `apply_seen` actually changed something with (`set_point(e1, Mar=80)`).
    pub fn take_applied(&mut self) -> Vec<String> {
        std::mem::take(&mut self.applied)
    }

    fn apply_inner(&mut self, seen: Option<&Scene>, ops: &[Op], chunk_id: u64) -> Option<Scene> {
        let mut applied = vec![];
        for op in ops {
            let changed = match (op, seen) {
                (Op::ClearBoard { .. }, Some(seen)) if seen.version != self.scene.version => {
                    let before = self.scene.elements.len();
                    self.scene.elements.retain(|e| !seen.elements.iter().any(|s| s.id == e.id));
                    let gone: Vec<String> = seen.elements.iter().map(|e| e.id.clone()).collect();
                    self.scene.annotations.retain(|a| !a.targets.iter().any(|t| gone.contains(t)));
                    if self.scene.elements.is_empty() {
                        self.scene.layout = Layout::Auto;
                    }
                    self.scene.elements.len() != before
                }
                _ => self.apply_one(op),
            };
            if changed {
                applied.push(op_name(op));
            }
        }
        self.applied = applied.clone();
        if applied.is_empty() {
            return None;
        }
        Some(self.bump(format!("agent:{}", applied.join(", ")), chunk_id))
    }

    fn miss(&mut self, why: String) -> bool {
        self.notes.push(why);
        false
    }

    fn chart_labels(&self, id: &str) -> String {
        self.scene.elements.iter().find(|e| e.id == id).and_then(|e| e.chart.as_ref()).map(|c| c.points.iter().map(|p| p.label.as_str()).collect::<Vec<_>>().join(", ")).unwrap_or_default()
    }

    fn has(&self, id: &str) -> bool {
        self.scene.elements.iter().any(|e| e.id == id)
    }

    fn text_blocks(&mut self, specs: &[TextBlockSpec], room: usize) -> Vec<TextBlock> {
        specs
            .iter()
            .filter_map(clean_text_spec)
            .take(room)
            .map(|b| TextBlock { id: self.new_block_id(), kind: b.kind, text: b.text, level: b.level, emphasis: b.emphasis })
            .collect()
    }

    fn apply_one(&mut self, op: &Op) -> bool {
        match op {
            Op::Focus { id } if self.has(id) => {
                for e in self.scene.elements.iter_mut() {
                    e.focus = &e.id == id;
                }
                true
            }
            Op::Remove { id, .. } if self.has(id) => {
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
            Op::ClearBoard { .. } if !self.scene.elements.is_empty() => {
                self.scene.elements.clear();
                self.scene.annotations.clear();
                self.scene.layout = Layout::Auto;
                true
            }
            Op::DrawDiagram { layout, title, nodes, edges } => {
                let mut d = Diagram { layout: *layout, title: title.as_deref().map(|t| short(t, 48)).filter(|t| !t.is_empty()), nodes: vec![], edges: vec![], auto_edges: false };
                d.add_nodes(nodes);
                // A process or history told step by step starts with its first step ("first we record audio",
                // 09-19: rejected, and the step was lost); cycles and hubs need at least two to mean anything.
                let min = if matches!(layout, DiagramLayout::Flow | DiagramLayout::Timeline) { 1 } else { 2 };
                if d.nodes.len() < min {
                    return false;
                }
                if d.add_edges(edges) == 0 {
                    d.auto_edges = true;
                    d.auto_link(0);
                }
                // Models often redraw a diagram instead of extending it: if one on the board shares
                // most of these nodes, replace it in place (keeps its tile and the animation calm).
                let labels: Vec<&str> = d.nodes.iter().map(|n| n.label.as_str()).collect();
                // …and a same-layout diagram while one is in focus is that explanation being reworked
                // ("it runs in a loop … we listen, we decide, we show" drew a 2nd cycle, 09-19).
                let twin = self.scene.elements.iter().position(|e| {
                    e.diagram.as_ref().is_some_and(|o| {
                        let shared = o.nodes.iter().filter(|n| labels.iter().any(|l| same(l, &n.label))).count();
                        shared * 2 >= o.nodes.len().max(2) || (e.focus && o.layout == d.layout)
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
                        self.push_element(ElementKind::Diagram, "", &caption, "", Some(d), None, None);
                    }
                }
                true
            }
            Op::AddNodes { id, nodes, edges } => {
                let Some(e) = self.scene.elements.iter_mut().find(|e| &e.id == id && e.diagram.is_some()) else { return self.miss(format!("add_nodes: no diagram {id}")) };
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
                self.push_element(ElementKind::Chart, "", &caption, "", None, Some(Chart { kind, title, unit, points }), None);
                true
            }
            Op::UpdateChart { id, kind, title, points } => {
                // A different kind of chart (bar → pie) or a data set sharing no labels with this chart is a
                // NEW chart, not an update: 09-19, "60% of them are students" turned the users bar chart
                // into a pie and wiped it (both Haiku and Gemini did this).
                if let Some(old) = self.scene.elements.iter().find(|e| &e.id == id).and_then(|e| e.chart.as_ref()) {
                    let pie = |k: ChartKind| k == ChartKind::Pie;
                    let kind_jump = kind.is_some_and(|k| pie(k) != pie(old.kind));
                    let fresh: Vec<Point> = clean_points(points).into_iter().filter(|p| !old.points.iter().any(|o| same(&o.label, &p.label) && o.value == p.value)).collect();
                    // related = shares a label or a value ("Stoned 60%" → "Students 60%" is a correction, not a new chart)
                    let unrelated = !old.points.is_empty() && !points.is_empty()
                        && !points.iter().any(|p| old.points.iter().any(|o| same(&o.label, &p.label) || o.value == p.value));
                    if kind_jump || unrelated {
                        let k = kind.unwrap_or(old.kind);
                        return !fresh.is_empty() && self.apply_one(&Op::DrawChart { kind: k, title: title.clone(), unit: None, points: fresh });
                    }
                }
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
            Op::SetPoint { id, label, value } => {
                let value = *value;
                let outcome = match self.scene.elements.iter_mut().find(|e| &e.id == id).and_then(|e| e.chart.as_mut()) {
                    None => Err(format!("set_point: no chart {id}")),
                    Some(_) if !value.is_finite() => Err(format!("set_point: {value} is not a number")),
                    Some(c) => match find_point(&c.points, label) {
                        None => Err(String::new()),
                        Some(i) if c.points[i].value == value => Ok(false),
                        Some(i) => {
                            c.points[i].value = value;
                            Ok(true)
                        }
                    },
                };
                match outcome {
                    Ok(changed) => {
                        if changed {
                            self.focus_only(id);
                        }
                        changed
                    }
                    Err(why) if why.is_empty() => {
                        let have = self.chart_labels(id);
                        self.miss(format!("set_point: no point like {label:?} in {id} (has: {have})"))
                    }
                    Err(why) => self.miss(why),
                }
            }
            Op::AddPoint { id, label, value, icon, .. } => {
                let (value, icon) = (*value, icon.clone().filter(|i| !i.is_empty()));
                let label = short(label, 24);
                let outcome = match self.scene.elements.iter_mut().find(|e| &e.id == id).and_then(|e| e.chart.as_mut()) {
                    None => Err(format!("add_point: no chart {id}")),
                    Some(_) if !value.is_finite() || label.is_empty() => Err(format!("add_point: bad point {label:?} {value}")),
                    // Already there → a correction, whatever the model called it.
                    Some(c) => match find_point(&c.points, &label) {
                        Some(i) => {
                            let changed = c.points[i].value != value;
                            c.points[i].value = value;
                            Ok(changed)
                        }
                        None if c.points.len() >= MAX_POINTS => Err(format!("add_point: {id} already has {MAX_POINTS} points")),
                        None => {
                            c.points.push(Point { label, value, icon, logo: None });
                            c.kind = promote(c.kind, c.points.len());
                            Ok(true)
                        }
                    },
                };
                match outcome {
                    Ok(changed) => {
                        if changed {
                            self.focus_only(id);
                        }
                        changed
                    }
                    Err(why) => self.miss(why),
                }
            }
            Op::RemovePoint { id, label, .. } => {
                let outcome = match self.scene.elements.iter_mut().find(|e| &e.id == id).and_then(|e| e.chart.as_mut()) {
                    None => Err(format!("remove_point: no chart {id}")),
                    Some(c) => match find_point(&c.points, label) {
                        None => Err(String::new()),
                        Some(_) if c.points.len() <= 1 => Err(format!("remove_point: {label:?} is the last point of {id}; remove the chart instead")),
                        Some(i) => {
                            c.points.remove(i);
                            Ok(())
                        }
                    },
                };
                match outcome {
                    Ok(()) => {
                        self.focus_only(id);
                        true
                    }
                    Err(why) if why.is_empty() => {
                        let have = self.chart_labels(id);
                        self.miss(format!("remove_point: no point like {label:?} in {id} (has: {have})"))
                    }
                    Err(why) => self.miss(why),
                }
            }
            Op::SetChart { id, kind, title, unit } => {
                let Some(e) = self.scene.elements.iter_mut().find(|e| &e.id == id && e.chart.is_some()) else { return self.miss(format!("set_chart: no chart {id}")) };
                let before = e.chart.clone();
                let c = e.chart.as_mut().unwrap();
                if let Some(k) = kind {
                    c.kind = promote(*k, c.points.len());
                }
                if let Some(t) = title.as_deref().map(|t| short(t, 48)).filter(|t| !t.is_empty()) {
                    c.title = Some(t.clone());
                    e.caption = t;
                }
                if let Some(u) = unit.as_deref() {
                    let u = short(u, 16);
                    c.unit = Some(u).filter(|u| !u.is_empty());
                }
                let changed = e.chart != before;
                if changed {
                    self.focus_only(id);
                }
                changed
            }
            Op::DrawText { blocks } => {
                let blocks = self.text_blocks(blocks, MAX_TEXT_BLOCKS);
                if blocks.is_empty() {
                    return self.miss("draw_text: no usable blocks".into());
                }
                let caption = blocks.iter().find(|b| b.kind == TextBlockKind::Heading).or_else(|| blocks.first()).map(|b| b.text.clone()).unwrap_or_default();
                // Text is the presenter’s current headline, not a board that accumulates. A new text card
                // replaces the newest existing one and removes any older text tiles left by earlier calls.
                let target = self.scene.elements.iter().rev().find(|e| e.kind == ElementKind::Text).map(|e| e.id.clone());
                if let Some(id) = target {
                    let removed: Vec<String> = self.scene.elements.iter().filter(|e| e.kind == ElementKind::Text && e.id != id).map(|e| e.id.clone()).collect();
                    self.scene.elements.retain(|e| e.kind != ElementKind::Text || e.id == id);
                    self.scene.annotations.retain(|a| !a.targets.iter().any(|t| removed.contains(t)));
                    let e = self.scene.elements.iter_mut().find(|e| e.id == id).unwrap();
                    e.caption = caption;
                    e.text = Some(TextCard { blocks });
                    self.focus_only(&id);
                } else {
                    self.push_element(ElementKind::Text, "", &caption, "", None, None, Some(TextCard { blocks }));
                }
                true
            }
            Op::AddTextBlocks { id, blocks } => {
                let Some(current) = self.scene.elements.iter().find(|e| &e.id == id).and_then(|e| e.text.as_ref()).map(|t| t.blocks.len()) else {
                    return self.miss(format!("add_text_blocks: no text tile {id}"));
                };
                let has_body = self.scene.elements.iter().find(|e| &e.id == id).and_then(|e| e.text.as_ref()).is_some_and(|t| t.blocks.iter().any(|b| b.kind == TextBlockKind::Paragraph));
                let replaces_body = has_body && blocks.iter().any(|b| b.kind != TextBlockKind::Heading);
                if current >= MAX_TEXT_BLOCKS && !replaces_body {
                    return self.miss(format!("add_text_blocks: {id} already has {MAX_TEXT_BLOCKS} blocks"));
                }
                let added = self.text_blocks(blocks, MAX_TEXT_BLOCKS);
                if added.is_empty() {
                    return self.miss("add_text_blocks: no usable blocks".into());
                }
                let card = self.scene.elements.iter_mut().find(|e| &e.id == id).unwrap().text.as_mut().unwrap();
                let mut changed = false;
                for block in added {
                    if block.kind == TextBlockKind::Paragraph || (has_body && block.kind == TextBlockKind::Bullet) {
                        if let Some(body) = card.blocks.iter_mut().find(|b| b.kind == TextBlockKind::Paragraph) {
                            changed |= body.text != block.text || body.level != block.level || body.emphasis != block.emphasis;
                            body.text = block.text;
                            body.level = 0;
                            body.emphasis = block.emphasis;
                            continue;
                        }
                    }
                    if card.blocks.len() < MAX_TEXT_BLOCKS {
                        card.blocks.push(block);
                        changed = true;
                    }
                }
                if changed { self.focus_only(id); }
                changed
            }
            Op::UpdateTextBlock { id, block, text, emphasis, level } => {
                let heading_only_body = self.scene.elements.iter().find(|e| &e.id == id).and_then(|e| e.text.as_ref()).is_some_and(|card| {
                    card.blocks.len() == 1 && card.blocks[0].id == *block && card.blocks[0].kind == TextBlockKind::Heading && *level == Some(0)
                });
                if heading_only_body {
                    let spec = TextBlockSpec {
                        kind: TextBlockKind::Paragraph,
                        text: text.clone().unwrap_or_default(),
                        level: 0,
                        emphasis: emphasis.clone().unwrap_or_default(),
                    };
                    let Some(clean) = clean_text_spec(&spec) else { return self.miss("update_text_block: no usable first body".into()) };
                    let body = TextBlock { id: self.new_block_id(), kind: clean.kind, text: clean.text, level: clean.level, emphasis: clean.emphasis };
                    self.scene.elements.iter_mut().find(|e| &e.id == id).unwrap().text.as_mut().unwrap().blocks.push(body);
                    self.focus_only(id);
                    return true;
                }
                let Some(card) = self.scene.elements.iter_mut().find(|e| &e.id == id).and_then(|e| e.text.as_mut()) else {
                    return self.miss(format!("update_text_block: no text tile {id}"));
                };
                let Some(i) = card.blocks.iter().position(|b| b.id == *block) else {
                    return self.miss(format!("update_text_block: no block {block} in {id}"));
                };
                let old = card.blocks[i].clone();
                let spec = TextBlockSpec {
                    kind: old.kind,
                    text: text.clone().unwrap_or_else(|| old.text.clone()),
                    level: level.unwrap_or(old.level),
                    emphasis: emphasis.clone().unwrap_or_else(|| old.emphasis.clone()),
                };
                let Some(clean) = clean_text_spec(&spec) else { return self.miss("update_text_block: empty text".into()) };
                card.blocks[i].text = clean.text;
                card.blocks[i].level = clean.level;
                card.blocks[i].emphasis = clean.emphasis;
                let changed = card.blocks[i] != old;
                if changed { self.focus_only(id); }
                changed
            }
            Op::RemoveTextBlock { id, block, .. } => {
                let Some(card) = self.scene.elements.iter_mut().find(|e| &e.id == id).and_then(|e| e.text.as_mut()) else {
                    return self.miss(format!("remove_text_block: no text tile {id}"));
                };
                if card.blocks.len() <= 1 {
                    return self.miss(format!("remove_text_block: {block} is the last block of {id}; remove the tile instead"));
                }
                let before = card.blocks.len();
                card.blocks.retain(|b| b.id != *block);
                if card.blocks.len() == before {
                    return self.miss(format!("remove_text_block: no block {block} in {id}"));
                }
                self.focus_only(id);
                true
            }
            Op::UpdateNode { id, node, label, note } => {
                let Some(d) = self.scene.elements.iter_mut().find(|e| &e.id == id).and_then(|e| e.diagram.as_mut()) else { return self.miss(format!("update_node: no diagram {id}")) };
                let Some(nid) = resolve(&d.nodes, node) else { return self.miss(format!("update_node: no node {node:?} in {id}")) };
                let taken = |d: &Diagram, l: &str| d.nodes.iter().any(|n| n.id != nid && same(&n.label, l));
                let mut changed = false;
                let n = d.nodes.iter().position(|n| n.id == nid).unwrap();
                if let Some(l) = label.as_deref().map(|l| short(l, 32)).filter(|l| !l.is_empty()) {
                    if taken(d, &l) {
                        self.notes.push(format!("update_node: another node in {id} is already called {l:?}"));
                    } else if d.nodes[n].label != l {
                        d.nodes[n].label = l;
                        changed = true;
                    }
                }
                if let Some(t) = note.as_deref() {
                    let t = Some(short(t, 24)).filter(|t| !t.is_empty());
                    if d.nodes[n].note != t {
                        d.nodes[n].note = t;
                        changed = true;
                    }
                }
                if changed {
                    self.focus_only(id);
                }
                changed
            }
            Op::RemoveNode { id, node, .. } => {
                let Some(d) = self.scene.elements.iter_mut().find(|e| &e.id == id).and_then(|e| e.diagram.as_mut()) else { return self.miss(format!("remove_node: no diagram {id}")) };
                let Some(nid) = resolve(&d.nodes, node) else { return self.miss(format!("remove_node: no node {node:?} in {id}")) };
                if d.nodes.len() <= 1 {
                    return self.miss(format!("remove_node: {node:?} is the last node of {id}; remove the diagram instead"));
                }
                d.nodes.retain(|n| n.id != nid);
                d.edges.retain(|e| e.from != nid && e.to != nid);
                if d.auto_edges {
                    d.edges.clear();
                    d.auto_link(0);
                }
                self.focus_only(id);
                true
            }
            Op::AddEdge { id, from, to, label } => {
                let Some(d) = self.scene.elements.iter_mut().find(|e| &e.id == id).and_then(|e| e.diagram.as_mut()) else { return self.miss(format!("add_edge: no diagram {id}")) };
                let n = d.add_edges(&[EdgeSpec { from: from.clone(), to: to.clone(), label: label.clone() }]);
                if n == 0 {
                    return self.miss(format!("add_edge: {from:?} → {to:?} not added to {id} (unknown node, same node, or already there)"));
                }
                d.auto_edges = false;
                self.focus_only(id);
                true
            }
            Op::RemoveEdge { id, from, to } => {
                let Some(d) = self.scene.elements.iter_mut().find(|e| &e.id == id).and_then(|e| e.diagram.as_mut()) else { return self.miss(format!("remove_edge: no diagram {id}")) };
                let (Some(f), Some(t)) = (resolve(&d.nodes, from), resolve(&d.nodes, to)) else { return self.miss(format!("remove_edge: unknown node in {from:?} → {to:?}")) };
                let before = d.edges.len();
                d.edges.retain(|e| !(e.from == f && e.to == t));
                if d.edges.len() == before {
                    return self.miss(format!("remove_edge: no edge {from:?} → {to:?} in {id}"));
                }
                d.auto_edges = false;
                self.focus_only(id);
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

/// A short label for logs: `set_point(e1, Mar=80)`.
pub fn op_name(op: &Op) -> String {
    match op {
        Op::Focus { id } => format!("focus({id})"),
        Op::Remove { id, .. } => format!("remove({id})"),
        Op::Arrange { layout } => format!("arrange({layout:?})").to_lowercase(),
        Op::Annotate { kind, .. } => format!("annotate({kind:?})").to_lowercase(),
        Op::ClearAnnotations => "clear_annotations".into(),
        Op::ClearBoard { .. } => "clear_board".into(),
        Op::DrawDiagram { layout, nodes, .. } => format!("draw_diagram({layout:?}, {} nodes)", nodes.len()).to_lowercase(),
        Op::AddNodes { id, nodes, .. } => format!("add_nodes({id}, +{})", nodes.len()),
        Op::UpdateNode { id, node, .. } => format!("update_node({id}, {node})"),
        Op::RemoveNode { id, node, .. } => format!("remove_node({id}, {node})"),
        Op::AddEdge { id, from, to, .. } => format!("add_edge({id}, {from}→{to})"),
        Op::RemoveEdge { id, from, to } => format!("remove_edge({id}, {from}→{to})"),
        Op::SetPoint { id, label, value } => format!("set_point({id}, {label}={value})"),
        Op::AddPoint { id, label, value, .. } => format!("add_point({id}, {label}={value})"),
        Op::RemovePoint { id, label, .. } => format!("remove_point({id}, {label})"),
        Op::SetChart { id, .. } => format!("set_chart({id})"),
        Op::DrawText { blocks } => format!("draw_text({} blocks)", blocks.len()),
        Op::AddTextBlocks { id, blocks } => format!("add_text_blocks({id}, +{})", blocks.len()),
        Op::UpdateTextBlock { id, block, .. } => format!("update_text_block({id}, {block})"),
        Op::RemoveTextBlock { id, block, .. } => format!("remove_text_block({id}, {block})"),
        Op::ShowPhoto { subject, replace } => format!("show_photo({subject}{})", if *replace { ", replace" } else { "" }),
        Op::ShowLogo { name, replace } => format!("show_logo({name}{})", if *replace { ", replace" } else { "" }),
        Op::ShowIcon { concept, replace, .. } => format!("show_icon({concept}{})", if *replace { ", replace" } else { "" }),
        Op::NoAction { .. } => "no_action".into(),
        Op::DrawChart { kind, points, .. } => format!("draw_chart({kind:?}, {} points)", points.len()).to_lowercase(),
        Op::UpdateChart { id, points, .. } => format!("update_chart({id}, {} points)", points.len()),
    }
}

/// One short line per tile, for Jev's `on_screen` (09-19: without it Jev couldn't see that a chart already
/// covered "200 users… 500…", and photo searches competed with the chart).
pub fn board_summary(scene: &Scene) -> Vec<String> {
    let n = scene.elements.len();
    let num = |v: f64| {
        let a = v.abs();
        let t = |x: f64| format!("{x:.1}").trim_end_matches(".0").to_string();
        if a >= 1e9 { format!("{}B", t(v / 1e9)) } else if a >= 1e6 { format!("{}M", t(v / 1e6)) } else if a >= 1e3 { format!("{}K", t(v / 1e3)) } else { t(v) }
    };
    scene
        .elements
        .iter()
        .map(|e| {
            let focus = if e.focus && n > 1 { " (in focus)" } else { "" };
            match (&e.diagram, &e.chart, &e.text) {
                (Some(d), _, _) => {
                    let labels: Vec<&str> = d.nodes.iter().map(|x| x.label.as_str()).collect();
                    let body = match d.layout {
                        DiagramLayout::Hub if labels.len() > 1 => format!("{} — {}", labels[0], labels[1..].join(", ")),
                        DiagramLayout::Cycle => format!("{} → (repeats)", labels.join(" → ")),
                        _ => labels.join(" → "),
                    };
                    format!("{:?} diagram{}: {body}{focus}", d.layout, d.title.as_deref().map(|t| format!(" '{t}'")).unwrap_or_default()).to_lowercase()
                }
                (_, Some(c), _) => {
                    let unit = c.unit.as_deref().map(|u| if u.trim() == "%" { "%".to_string() } else { format!(" {u}") }).unwrap_or_default();
                    let pts: Vec<String> = c.points.iter().map(|p| format!("{} {}{unit}", p.label, num(p.value))).collect();
                    format!("{:?} chart{}: {}{focus}", c.kind, c.title.as_deref().map(|t| format!(" '{t}'")).unwrap_or_default(), pts.join(", ")).to_lowercase()
                }
                (_, _, Some(t)) => format!("text: {}{focus}", t.blocks.iter().map(|b| format!("{}={}", b.id, b.text)).collect::<Vec<_>>().join(" | ")),
                _ if e.kind == ElementKind::Logo => format!("{}: {}{focus}", if e.image_id.is_empty() { "name card" } else { "logo" }, e.caption),
                _ => format!("photo: {}{focus}", e.caption.split('(').next().unwrap_or(&e.caption).trim()),
            }
        })
        .collect()
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

/// The presenter asks for something to be taken off the screen — the only time the agent may remove a tile
/// (09-19: Gemini "tidied" the board, removing photos 0.6 s after they appeared).
pub fn has_removal_cue(text: &str) -> bool {
    let t = text.to_lowercase().replace('’', "'");
    [
        "remove", "get rid of", "take away", "take that away", "take it away", "take that down", "take it down", "put that aside",
        "put it aside", "set aside", "set that aside", "forget the", "forget about", "drop the", "hide the", "don't need the",
        "no longer need",
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
        return vec![Op::ClearBoard { quote: None }];
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
        // an older version no longer matters: ops address tiles by id and apply if their target exists
        let stale = c.apply(s.version - 1, &[Op::Arrange { layout: Layout::Compare }], 3).expect("arrange applies on a changed board");
        assert_eq!(stale.layout, Layout::Compare);
        let s = c.scene().clone();
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
        let out = c.apply(out.version, &[Op::Remove { id: ids[0].clone(), quote: None }], 4).unwrap();
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
        labels.iter().map(|l| NodeSpec { label: l.to_string(), icon: None, logo: None, note: None }).collect()
    }
    fn pts(v: &[(&str, f64)]) -> Vec<Point> {
        v.iter().map(|(l, x)| Point { label: l.to_string(), value: *x, icon: None, logo: None }).collect()
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
        let s = c.apply(s.version, &[Op::AddNodes { id: id.clone(), nodes: ns(&["Decide", "Show"]), edges: vec![] }], 2).unwrap();
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
        let s = c.apply(s.version, &[Op::AddNodes { id, nodes: ns(&["Learn"]), edges: vec![] }], 2).unwrap();
        let d = s.elements[0].diagram.as_ref().unwrap();
        assert!(d.edges.iter().any(|e| e.from == "n4" && e.to == "n1") && !d.edges.iter().any(|e| e.from == "n3" && e.to == "n1"), "{:?}", d.edges);
        let s = c.apply(s.version, &[Op::DrawDiagram { layout: DiagramLayout::Flow, title: None, nodes: ns(&["Rain", "Flood", "Drought"]),
            edges: vec![EdgeSpec { from: "rain".into(), to: "Flood".into(), label: Some("too much".into()) }, EdgeSpec { from: "x".into(), to: "Flood".into(), label: None }] }], 3).unwrap();
        let d = s.elements[1].diagram.as_ref().unwrap();
        assert_eq!(d.edges.len(), 1);
        assert!(!d.auto_edges);
        assert!(c.apply(s.version, &[Op::DrawDiagram { layout: DiagramLayout::Hub, title: None, nodes: ns(&["alone"]), edges: vec![] }], 4).is_none(), "one node is not a diagram");
    }

    fn chart(c: &mut Canvas, title: &str, points: &[(&str, f64)]) -> String {
        c.apply(0, &[Op::DrawChart { kind: ChartKind::Bar, title: Some(title.into()), unit: None, points: pts(points) }], 1).unwrap().elements.last().unwrap().id.clone()
    }
    fn points_of(c: &Canvas, id: &str) -> Vec<(String, f64)> {
        c.scene().elements.iter().find(|e| e.id == id).unwrap().chart.as_ref().unwrap().points.iter().map(|p| (p.label.clone(), p.value)).collect()
    }
    fn v(pairs: &[(&str, f64)]) -> Vec<(String, f64)> {
        pairs.iter().map(|(l, x)| (l.to_string(), *x)).collect()
    }

    #[test]
    fn set_point_corrects_one_value_and_matches_labels_loosely() {
        let mut c = Canvas::new();
        let id = chart(&mut c, "Users", &[("January", 40.0), ("February", 55.0), ("March", 70.0)]);
        // "Mar" is March; the other bars are untouched
        let s = c.apply(0, &[Op::SetPoint { id: id.clone(), label: "Mar".into(), value: 80.0 }], 2).unwrap();
        assert!(s.reason.contains("set_point"));
        assert_eq!(points_of(&c, &id), v(&[("January", 40.0), ("February", 55.0), ("March", 80.0)]));
        assert!(c.apply(0, &[Op::SetPoint { id: id.clone(), label: "march".into(), value: 80.0 }], 2).is_none(), "same value: no change");
        // an unknown label is refused, with a reason, and changes nothing
        assert!(c.apply(0, &[Op::SetPoint { id: id.clone(), label: "December".into(), value: 1.0 }], 2).is_none());
        let notes = c.take_notes();
        assert!(notes.iter().any(|n| n.contains("December") && n.contains("January")), "{notes:?}");
        assert!(c.take_notes().is_empty(), "notes drain");
        assert_eq!(points_of(&c, &id).len(), 3);
    }

    #[test]
    fn add_and_remove_point_and_set_chart() {
        let mut c = Canvas::new();
        let stat = c.apply(0, &[Op::DrawChart { kind: ChartKind::Stat, title: Some("Revenue".into()), unit: None, points: pts(&[("Last year", 15000.0)]) }], 1).unwrap().elements[0].id.clone();
        let s = c.apply(0, &[Op::AddPoint { id: stat.clone(), label: "Year before".into(), value: 12000.0, icon: None, logo: None }], 2).unwrap();
        assert_eq!(s.elements[0].chart.as_ref().unwrap().kind, ChartKind::Bar, "a second value turns a stat into bars");
        // add_point on a label that exists is a correction, not a duplicate bar
        c.apply(0, &[Op::AddPoint { id: stat.clone(), label: "last year".into(), value: 16000.0, icon: None, logo: None }], 3).unwrap();
        assert_eq!(points_of(&c, &stat), v(&[("Last year", 16000.0), ("Year before", 12000.0)]));
        c.apply(0, &[Op::RemovePoint { id: stat.clone(), label: "Year before".into(), quote: Some("drop it".into()) }], 4).unwrap();
        assert_eq!(points_of(&c, &stat), v(&[("Last year", 16000.0)]));
        assert!(c.apply(0, &[Op::RemovePoint { id: stat.clone(), label: "Last year".into(), quote: None }], 5).is_none(), "the last point stays");
        assert!(c.take_notes().iter().any(|n| n.contains("last point")));
        let s = c.apply(0, &[Op::SetChart { id: stat.clone(), kind: Some(ChartKind::Line), title: Some("Signups".into()), unit: Some("users".into()) }], 6).unwrap();
        let ch = s.elements[0].chart.as_ref().unwrap();
        assert_eq!((ch.kind, ch.title.as_deref(), ch.unit.as_deref()), (ChartKind::Line, Some("Signups"), Some("users")));
        assert_eq!(s.elements[0].caption, "Signups");
        // a full chart takes no more points
        let full: Vec<(String, f64)> = (0..MAX_POINTS).map(|i| (format!("p{i}"), i as f64)).collect();
        let refs: Vec<(&str, f64)> = full.iter().map(|(l, x)| (l.as_str(), *x)).collect();
        let id = chart(&mut c, "Full", &refs);
        assert!(c.apply(0, &[Op::AddPoint { id, label: "extra".into(), value: 99.0, icon: None, logo: None }], 7).is_none());
    }

    #[test]
    fn diagram_nodes_can_be_renamed_removed_and_relinked() {
        let mut c = Canvas::new();
        let id = c.apply(0, &[Op::DrawDiagram { layout: DiagramLayout::Flow, title: None, nodes: ns(&["Commit", "Build", "Test", "Deploy"]), edges: vec![] }], 1).unwrap().elements[0].id.clone();
        let labels = |c: &Canvas| c.scene().elements[0].diagram.as_ref().unwrap().nodes.iter().map(|n| n.label.clone()).collect::<Vec<_>>();
        c.apply(0, &[Op::UpdateNode { id: id.clone(), node: "build".into(), label: Some("Compile".into()), note: None }], 2).unwrap();
        assert_eq!(labels(&c), ["Commit", "Compile", "Test", "Deploy"]);
        // removing a node re-chains the rest
        c.apply(0, &[Op::RemoveNode { id: id.clone(), node: "Test".into(), quote: Some("take the test step out".into()) }], 3).unwrap();
        let d = c.scene().elements[0].diagram.as_ref().unwrap();
        assert_eq!(d.nodes.len(), 3);
        assert_eq!(d.edges.len(), 2, "chain closed over the gap");
        // a new node never reuses the id of a removed one
        c.apply(0, &[Op::AddNodes { id: id.clone(), nodes: ns(&["Monitor"]), edges: vec![] }], 4).unwrap();
        let ids: Vec<String> = c.scene().elements[0].diagram.as_ref().unwrap().nodes.iter().map(|n| n.id.clone()).collect();
        assert_eq!(ids.len(), ids.iter().collect::<std::collections::HashSet<_>>().len(), "unique node ids: {ids:?}");
        // an unknown node is refused with a reason
        assert!(c.apply(0, &[Op::UpdateNode { id: id.clone(), node: "Nope".into(), label: Some("X".into()), note: None }], 5).is_none());
        assert!(c.take_notes().iter().any(|n| n.contains("Nope")));
        c.apply(0, &[Op::AddEdge { id: id.clone(), from: "Commit".into(), to: "Monitor".into(), label: Some("also".into()) }], 6).unwrap();
        c.apply(0, &[Op::RemoveEdge { id, from: "Commit".into(), to: "Monitor".into() }], 7).unwrap();
    }

    #[test]
    fn removes_and_clears_apply_by_id_on_a_changed_board() {
        let mut c = Canvas::new();
        let a = add(&mut c, "eagle");
        add(&mut c, "owl");
        let seen = c.scene().clone();
        // while the agent thinks, a photo lands and the board version moves on
        add(&mut c, "parrot");
        let owl = seen.elements[1].id.clone();
        c.apply_seen(&seen, &[Op::Remove { id: owl.clone(), quote: Some("take the owl away".into()) }], 5).unwrap();
        assert!(!c.scene().elements.iter().any(|e| e.id == owl), "remove applies although the board changed");
        // an id that is already gone is refused, not applied to something else
        assert!(c.apply_seen(&seen, &[Op::Remove { id: owl, quote: None }], 6).is_none());
        // clear removes what the agent saw, and only that
        let s = c.apply_seen(&seen, &[Op::ClearBoard { quote: Some("move on".into()) }], 7).unwrap();
        assert_eq!(s.elements.len(), 1, "the photo that arrived meanwhile survives");
        assert_eq!(s.elements[0].image_id, "parrot");
        assert!(!s.elements.iter().any(|e| e.id == a.elements[0].id));
    }

    #[test]
    fn logos_and_name_cards_are_tiles_that_dedupe() {
        let mut c = Canvas::new();
        add(&mut c, "eagle");
        let s = c.render_logo("logos:google-icon", "Google", "img://localhost/logos-google-icon.svg", 2);
        assert_eq!(s.elements.len(), 2);
        let e = s.elements.last().unwrap();
        assert_eq!((e.kind, e.image_id.as_str(), e.focus), (ElementKind::Logo, "logos:google-icon", true));
        // the same asset again only refocuses; a name card dedupes on its (case-insensitive) name
        add(&mut c, "owl");
        let s = c.render_logo("logos:google-icon", "Google", "img://x", 3);
        assert_eq!(s.elements.len(), 3);
        assert!(s.elements.iter().find(|e| e.image_id == "logos:google-icon").unwrap().focus);
        c.render_logo("", "Hooli", "", 4);
        let s = c.render_logo("", "hooli", "", 5);
        assert_eq!(s.elements.iter().filter(|e| e.kind == ElementKind::Logo && e.image_id.is_empty()).count(), 1);
        let sum = board_summary(&s);
        assert!(sum.iter().any(|l| l.starts_with("logo: Google")) && sum.iter().any(|l| l.starts_with("name card: ")), "{sum:?}");
        // the oldest tile is evicted past four, like any other
        assert_eq!(s.elements.len(), 4);
    }

    #[test]
    fn a_refined_symbol_replaces_the_one_just_shown() {
        let mut c = Canvas::new();
        add(&mut c, "eagle");
        c.render_logo("lucide:flag", "Flag", "img://x/flag.svg", 2);
        let s = c.replace_logo("circle-flags:ca", "Canada", "img://x/ca.svg", 3);
        assert_eq!(s.elements.len(), 2, "swapped in place, not added");
        let e = s.elements.iter().find(|e| e.kind == ElementKind::Logo).unwrap();
        assert_eq!((e.image_id.as_str(), e.caption.as_str(), e.focus), ("circle-flags:ca", "Canada", true));
        // a name card can be refined into a logo, and replacing with none on the board just adds
        let mut d = Canvas::new();
        assert_eq!(d.replace_logo("", "Hooli", "", 1).elements.len(), 1);
        assert_eq!(d.replace_logo("logos:google-icon", "Google", "img://g", 2).elements.len(), 1);
    }

    #[test]
    fn charts_draw_merge_and_survive_a_changed_board() {
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
        assert_eq!(s.layout, Layout::Grid, "an arrange from an older board still applies");
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

    /// 09-19, the recorded failure: "last year we had like 200 users" drew a 1-point stat, then "the year
    /// before that we had 300 users" updated it — still `stat`, which both renderers draw as two numbers
    /// joined by an arrow. Two spoken quantities are a comparison, and a comparison is bars. This goes
    /// through the UpdateChart caller of `promote`, which is the path the incident actually took.
    #[test]
    fn two_spoken_numbers_are_bars_even_when_the_model_says_stat() {
        let mut c = Canvas::new();
        let v = c.scene().version;
        let s = c
            .apply(v, &[Op::DrawChart { kind: ChartKind::Stat, title: Some("Users last year".into()), unit: Some("users".into()), points: pts(&[("Last year", 200.0)]) }], 6)
            .unwrap();
        let e = s.elements.last().unwrap();
        assert_eq!(e.chart.as_ref().unwrap().kind, ChartKind::Stat, "one number is still a headline number");
        let id = e.id.clone();
        let s = c
            .apply(
                s.version,
                &[Op::UpdateChart { id, kind: Some(ChartKind::Stat), title: Some("Users over two years".into()), points: pts(&[("Year before", 300.0), ("Last year", 200.0)]) }],
                7,
            )
            .unwrap();
        let ch = s.elements.last().unwrap().chart.as_ref().unwrap();
        assert_eq!((ch.kind, ch.points.len()), (ChartKind::Bar, 2), "a second value makes it bars even when the model insists on stat");
    }

    #[test]
    fn board_summary_lists_every_tile() {
        let mut c = Canvas::new();
        add(&mut c, "penguin");
        let s = c.apply(c.scene().version, &[Op::DrawChart { kind: ChartKind::Bar, title: Some("Users".into()), unit: Some("users".into()),
            points: pts(&[("Last year", 200.0), ("This year", 1500.0)]) }], 2).unwrap();
        let s = c.apply(s.version, &[Op::DrawDiagram { layout: DiagramLayout::Cycle, title: None, nodes: ns(&["Listen", "Decide", "Show"]), edges: vec![] }], 3).unwrap();
        assert_eq!(board_summary(&s), vec![
            "photo: penguin".to_string(),
            "bar chart 'users': last year 200 users, this year 1.5k users".to_string(),
            "cycle diagram: listen → decide → show → (repeats) (in focus)".to_string(),
        ]);
    }

    #[test]
    fn a_new_breakdown_is_a_new_chart_and_reworks_stay_in_one_diagram() {
        let mut c = Canvas::new();
        let s = c.apply(0, &[Op::DrawChart { kind: ChartKind::Bar, title: Some("Users".into()), unit: None, points: pts(&[("Last year", 2000.0), ("This year", 15000.0)]) }], 1).unwrap();
        let users = s.elements[0].id.clone();
        // the model "updates" the users chart into a pie that keeps the old points plus a new share
        let s = c.apply(s.version, &[Op::UpdateChart { id: users.clone(), kind: Some(ChartKind::Pie), title: Some("Who".into()),
            points: pts(&[("Last year", 2000.0), ("This year", 15000.0), ("Students", 60.0)]) }], 2).unwrap();
        assert_eq!(s.elements.len(), 2, "pie is a new tile");
        assert_eq!(s.elements[0].chart.as_ref().unwrap().points.len(), 2, "users chart untouched");
        assert_eq!(s.elements[1].chart.as_ref().unwrap().points, pts(&[("Students", 60.0)]), "old series not copied into the pie");
        // a misheard label corrected (same value) stays the same chart
        let pie_id = s.elements[1].id.clone();
        let s = c.apply(s.version, &[Op::UpdateChart { id: pie_id, kind: None, title: None, points: pts(&[("Students", 60.0), ("Teachers", 30.0), ("Parents", 10.0)]) }], 2).unwrap();
        assert_eq!(s.elements.len(), 2, "correction in place");
        assert_eq!(s.elements[1].chart.as_ref().unwrap().points.len(), 3);
        let users = s.elements[0].id.clone();
        // a data set with no labels or values in common is also a new chart
        let s = c.apply(s.version, &[Op::UpdateChart { id: users, kind: None, title: None, points: pts(&[("Paris", 3.0), ("Tokyo", 5.0)]) }], 3).unwrap();
        assert_eq!(s.elements.len(), 3);
        // diagrams: a same-layout redraw while a diagram is in focus replaces it
        let s = c.apply(s.version, &[Op::DrawDiagram { layout: DiagramLayout::Cycle, title: None, nodes: ns(&["Speak", "Transcribe", "Decide", "Show"]), edges: vec![] }], 4).unwrap();
        let n = s.elements.len();
        let s = c.apply(s.version, &[Op::DrawDiagram { layout: DiagramLayout::Cycle, title: Some("How it works".into()), nodes: ns(&["Listen", "Decide", "Show"]), edges: vec![] }], 5).unwrap();
        assert_eq!(s.elements.len(), n, "reworked in place");
        assert_eq!(s.elements.iter().find(|e| e.focus).unwrap().diagram.as_ref().unwrap().nodes.len(), 3);
    }

    #[test]
    fn a_flow_can_start_with_its_first_step() {
        let mut c = Canvas::new();
        let s = c.apply(0, &[Op::DrawDiagram { layout: DiagramLayout::Flow, title: Some("Our pipeline".into()), nodes: ns(&["Record audio"]), edges: vec![] }], 1).unwrap();
        let id = s.elements[0].id.clone();
        let s = c.apply(s.version, &[Op::AddNodes { id, nodes: ns(&["Transcribe"]), edges: vec![] }], 2).unwrap();
        let d = s.elements[0].diagram.as_ref().unwrap();
        assert_eq!((d.nodes.len(), d.edges.len()), (2, 1), "the chain continues from the first step");
        assert!(c.apply(s.version, &[Op::DrawDiagram { layout: DiagramLayout::Cycle, title: None, nodes: ns(&["Alone"]), edges: vec![] }], 3).is_none());
    }

    #[test]
    fn text_blocks_draw_patch_and_remove_by_stable_id() {
        let block = |kind, text: &str, emphasis: &[&str]| TextBlockSpec {
            kind, text: text.into(), level: if kind == TextBlockKind::Heading { 1 } else { 0 }, emphasis: emphasis.iter().map(|s| s.to_string()).collect(),
        };
        let mut c = Canvas::new();
        let s = c.apply(0, &[Op::DrawText { blocks: vec![
            block(TextBlockKind::Heading, "Three lessons", &["Three"]),
            block(TextBlockKind::Bullet, "Start with the user", &["the user", "missing words"]),
        ]}], 1).unwrap();
        let e = &s.elements[0];
        assert_eq!(e.kind, ElementKind::Text);
        let id = e.id.clone();
        let first = e.text.as_ref().unwrap().blocks[0].id.clone();
        let second = e.text.as_ref().unwrap().blocks[1].id.clone();
        assert_eq!(e.text.as_ref().unwrap().blocks[1].emphasis, vec!["the user"], "emphasis must be an exact phrase in the block");

        let s = c.apply(s.version, &[Op::AddTextBlocks { id: id.clone(), blocks: vec![block(TextBlockKind::Bullet, "Measure the outcome", &["Measure the outcome"])] }], 2).unwrap();
        assert_eq!(s.elements[0].text.as_ref().unwrap().blocks.len(), 3);
        let s = c.apply(s.version, &[Op::UpdateTextBlock { id: id.clone(), block: second.clone(), text: Some("Start with real users".into()), emphasis: Some(vec!["real users".into()]), level: None }], 3).unwrap();
        assert_eq!(s.elements[0].text.as_ref().unwrap().blocks[1].text, "Start with real users");
        let s = c.apply(s.version, &[Op::RemoveTextBlock { id, block: first, quote: Some("remove the title".into()) }], 4).unwrap();
        assert_eq!(s.elements[0].text.as_ref().unwrap().blocks.len(), 2);

        let old_id = s.elements[0].id.clone();
        let s = c.apply(s.version, &[Op::DrawText { blocks: vec![block(TextBlockKind::Heading, "A new headline", &["new headline"])] }], 5).unwrap();
        let texts: Vec<&Element> = s.elements.iter().filter(|e| e.kind == ElementKind::Text).collect();
        assert_eq!(texts.len(), 1, "new text replaces every previous text tile");
        assert_eq!(texts[0].id, old_id, "the newest text tile is updated in place");
        assert_eq!(texts[0].text.as_ref().unwrap().blocks[0].text, "A new headline");

        assert!(c.apply(s.version, &[Op::DrawText { blocks: vec![block(TextBlockKind::Heading, "Unselected words", &[])] }], 6).is_none());
        assert!(c.take_notes().iter().any(|note| note.contains("draw_text: no usable blocks")));
    }

    #[test]
    fn a_section_keeps_one_stable_body_paragraph() {
        let block = |kind, text: &str, emphasis: &[&str]| TextBlockSpec {
            kind, text: text.into(), level: if kind == TextBlockKind::Heading { 1 } else { 0 }, emphasis: emphasis.iter().map(|s| s.to_string()).collect(),
        };
        let mut c = Canvas::new();
        let s = c.apply(0, &[Op::DrawText { blocks: vec![
            block(TextBlockKind::Heading, "The scenario", &["scenario"]),
            block(TextBlockKind::Paragraph, "A team loses Friday to reporting", &["loses Friday"]),
        ]}], 1).unwrap();
        let id = s.elements[0].id.clone();
        let body_id = s.elements[0].text.as_ref().unwrap().blocks[1].id.clone();
        let s = c.apply(s.version, &[Op::AddTextBlocks { id, blocks: vec![
            block(TextBlockKind::Paragraph, "They copy numbers across five spreadsheets", &["five spreadsheets"]),
        ]}], 2).unwrap();
        let blocks = &s.elements[0].text.as_ref().unwrap().blocks;
        assert_eq!(blocks.len(), 2, "a developed section still has one heading and one body");
        assert_eq!(blocks[1].id, body_id, "the body is patched rather than appended");
        assert!(blocks[1].text.contains("five spreadsheets"));

        let s = c.apply(s.version, &[Op::AddTextBlocks { id: s.elements[0].id.clone(), blocks: vec![
            block(TextBlockKind::Bullet, "They repeat it every Friday", &["every Friday"]),
        ]}], 3).unwrap();
        let blocks = &s.elements[0].text.as_ref().unwrap().blocks;
        assert_eq!(blocks.len(), 2, "prose mislabeled as a bullet cannot create a third section block");
        assert_eq!(blocks[1].kind, TextBlockKind::Paragraph);
        assert_eq!(blocks[1].id, body_id);

        let mut title_only = Canvas::new();
        let s = title_only.apply(0, &[Op::DrawText { blocks: vec![
            block(TextBlockKind::Heading, "The scenario", &["scenario"]),
        ]}], 1).unwrap();
        let id = s.elements[0].id.clone();
        let heading = s.elements[0].text.as_ref().unwrap().blocks[0].id.clone();
        let s = title_only.apply(s.version, &[Op::UpdateTextBlock {
            id, block: heading, text: Some("A team copies five spreadsheets".into()), emphasis: Some(vec!["five spreadsheets".into()]), level: Some(0),
        }], 2).unwrap();
        let blocks = &s.elements[0].text.as_ref().unwrap().blocks;
        assert_eq!(blocks.len(), 2, "body prose aimed at a lone heading becomes its first paragraph");
        assert_eq!(blocks[0].text, "The scenario");
        assert_eq!(blocks[1].kind, TextBlockKind::Paragraph);
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
        assert_eq!(rule_ops("ok, moving on", &s), vec![Op::ClearBoard { quote: None }]);
        assert!(rule_ops("they live in forests", &s).is_empty());
    }
}
