//! Canvas agent (Luna, `openai/gpt-5.6-luna`): the one decision-maker for what is on the board. Each call gets the
//! whole transcript, the board as it is now, what was changed recently and the newest words; it answers with tool
//! calls (show a photo, draw or patch a chart or diagram, remove, arrange, clear) or `no_action`. Ops go through
//! [`ground`] (spoken numbers only, quoted authority for destructive ops) before the canvas applies them.
//! No key / error / timeout → offline cue rules.
//!
//! Two backends, same tools and parsing: OpenAI's Responses WebSocket when `OPENAI_API_KEY` is set (with Chat
//! Completions fallback), OpenRouter otherwise. Their request shapes differ at the transport boundary.

use futures_util::{SinkExt, StreamExt};
use ls_canvas::{rule_ops, AnnotationKind, ChartKind, DiagramLayout, EdgeSpec, ElementKind, Layout, NodeSpec, Op, Point, Scene, TextBlockKind, TextBlockSpec};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, http::header, http::HeaderValue, Message};
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

/// GPT-5.6 Luna (user's choice, 09-19), named the OpenRouter way. OpenAI's own API calls it `gpt-5.6-luna`
/// (the `openai/` prefix is dropped). Reasoning is off/minimal: this call is a small tool decision, not a puzzle.
/// Alternatives on OpenRouter: `CANVAS_MODEL=anthropic/claude-haiku-4.5`, `google/gemini-2.5-flash`.
pub const DEFAULT_MODEL: &str = "openai/gpt-5.6-luna";

fn base() -> String {
    std::env::var("OPENROUTER_BASE_URL").unwrap_or_else(|_| "https://openrouter.ai".into())
}

/// OpenAI base URL; override with `OPENAI_BASE_URL` (e.g. a local mock).
fn openai_base() -> String {
    std::env::var("OPENAI_BASE_URL").unwrap_or_else(|_| "https://api.openai.com".into())
}

/// Which service a call goes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    OpenAi,
    OpenRouter,
}

/// How Luna was reached for a proposal. Logged separately from `Source` so a WebSocket failure followed by a
/// successful HTTP retry is visible without being counted as an offline fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    ResponsesWebSocket,
    ChatHttp,
    Rules,
}

/// Transcript budget in characters (~10k tokens). Older sentences past it are dropped from the front.
const TRANSCRIPT_CHARS: usize = 40_000;

/// The generic icon names Luna may use (each is an exact name in the icon library), one source for the prompt and the probes.
pub const ICON_NAMES: &str = include_str!("icon_names.txt");

/// The system prompt, with the icon vocabulary filled in.
fn system_prompt() -> String {
    SYSTEM.replace("{ICON_NAMES}", ICON_NAMES.trim())
}

const SYSTEM: &str = r#"You are the live designer for a talk. You are the only thing that changes the screen behind the presenter: you choose every photo, chart, diagram, removal, layout change and clear. You are called as the presenter speaks.

WHAT YOU GET EACH CALL
- Transcript: everything said BEFORE the newest words, oldest first, each sentence with the time it was said. It is CONTEXT: what the talk is about, what "that" or "it" refers to.
- Board: what is on screen right now, with ids (tiles e1, e2…; diagram nodes n1, n2…). It is the truth. Use its ids and labels exactly; never invent an id.
- Recent changes: what has already been done to the board, newest last. Never repeat one.
- Newest words: the sentences said since your last call, and the phrase being spoken right now (it may be unfinished, and early words can be misheard). ACT ONLY ON THESE. Never add, redraw or remove something just because it was said earlier: earlier speech was already handled or was not meant for the screen.

Call one or more tools. If nothing should change, call no_action with a short reason. That is the right answer most of the time: filler, greetings, opinions, an unfinished sentence, something already on screen, or a thing that is only mentioned and not meant to be shown.

WHAT THE PRESENTER WANTS → WHAT TO CALL
- SHOW something concrete (an animal, object, place, person, scene): show_photo. A company, product or brand ("the Google logo", "a logo of the company called Google", "let's put up Slack"): show_logo. A generic symbol for an idea or thing ("an icon for teamwork", "a database", "security", a country's flag): show_icon. Quantities they state: draw_chart. Steps, a process, a cycle, parts of a whole, dated events: draw_diagram.
- CORRECT a value or label that is already on the board: "actually it's 80", "let's correct March to eighty", "I meant February was sixty", "make that ninety", "it's not forty, it's forty five", "sorry, that should be…", "change X to Y". Chart → set_point. Diagram step → update_node. Correct exactly the thing they name. If they name nothing, it is the thing they just talked about: the chart or diagram in focus, or the one Recent changes shows was touched last. Never redraw a chart or diagram to correct one value.
- ADD to what is there: "and in April we hit ninety five" → add_point. "and then we monitor it" → add_nodes.
- REMOVE PART of a chart or diagram: "drop February", "take March out", "skip the test step" → remove_point / remove_node.
- RENAME or RESTYLE: "call this chart monthly signups", "show that as a line chart" → set_chart. "call the second step compile" → update_node.
- REMOVE A WHOLE TILE: "take the eagle away", "get rid of the chart" → remove.
- LAYOUT: compare two things → arrange compare; zoom in on one → focus that tile, then arrange hero; everything together → arrange grid; draw attention to something on a tile ("pay attention to the owl", "notice the eyes", "look at this part") → annotate highlight on that tile (an annotation, not just a focus: focus is for "zoom in" and "let's talk about this one"); link two tiles → annotate arrow.
- CLEAR: "let's move on", "next topic", "new section", "start fresh", "clear the screen" → clear_board (skip it if the board is already empty). "Move on" as a figure of speech ("many people move on from Java") is not a command.

PHOTOS (show_photo)
- subject = the EXACT thing named, 1–4 words: "owl", not "bird"; "white rose", not "flower". mode "add" (default) puts it next to what is there; mode "replace" only when they want the photo on screen swapped for a variant of the same subject ("make that the white one", "actually in red").
- One photo per call, except comparisons: "an eagle and an owl" is two show_photo calls.
- No photo when the thing is only a comparison or figure of speech ("watch like an eagle", "as fast as lightning"), when it is already on screen (as a photo, or covered by a chart or diagram), or when it is not a photographable thing: a logo or brand (show_logo), a symbol or idea (show_icon), a product screenshot, chart, diagram or text.
- The board holds 4 tiles and makes room by itself. Never remove a photo to tidy up.

LOGOS AND ICONS (show_logo, show_icon) come from a library searched by NAME, not by what things look like.
- show_logo(name): a company, product or brand the presenter asks to see, or that the whole presentation or section is ABOUT ("let's make this presentation about a company called Google", "here's the Slack logo"). A company that is merely the subject of a sentence with facts or figures ("Amazon Web Services has thirty two percent…") is not a request for its logo: that is a chart, and its logo goes on the chart's point. name = just the brand: "Google", "Microsoft Azure". If the library has no such logo a plain card with the name is shown, so asking is always safe.
- show_icon(concept, alternatives): a generic symbol: "teamwork", "a database", "security", "growth", or a country's flag ("flag of Canada"). concept = the idea in 1–2 words. alternatives = 2–4 other words a symbol could be named by, especially the plain object ("teamwork" → users, group, people; "growth" → trending up, sprout): the library knows names like "users" and "database", not every idea. A card with the concept is shown if nothing fits.
- The newest words can name the symbol more exactly than what you showed a moment ago (Recent changes shows a generic flag, and the words now say "the flag of Canada"; or a brand name that was cut off): call show_logo / show_icon again with mode "replace" instead of leaving the wrong one up.
- A company mentioned in passing ("Google announced new results", "unlike Microsoft") does not need a logo. (This is only about logos and icons: charts and diagrams are drawn from what is said, with no request needed.) Do not use show_photo for a brand or a symbol, and never a photo of a logo.

CHARTS — only from numbers the presenter actually says; never invent or estimate data.
- bar: values over time or across groups (the default whenever more numbers may follow); line: a trend over 3+ times; pie: shares of a whole; stat: exactly one number that stands alone.
- A large standalone magnitude is chart data even inside a narrative claim. For one number with a currency, percent, or scale word such as thousand, million or billion, use `draw_chart` with `kind: stat` rather than showing the amount only in text. Example: “McDonald’s lost over one hundred million dollars” → one `$100M` loss stat. Keep the surrounding explanation in text when it is useful.
- Numbers arrive one at a time. If they are listing or comparing values ("last year… the year before…"), use bar from the FIRST number: a bar chart with one value draws as one big number and grows into bars as more arrive. A stat becomes bars by itself when a second value is added.
- Plain numbers in `value`: "fifteen thousand" → 15000, "60 percent" → 60 with unit "%". If a chart is kept in millions and they say "eighteen million", the value is 18: match the scale the chart already uses.
- Never add an "Other", "Rest" or "Not X" remainder; a pie may sum to less than 100.
- Keep one chart per series: grow it with add_point and correct it with set_point, never a second draw_chart for the same numbers. A new set of numbers (for example shares of a whole after a growth trend) is a new chart. A correction never turns an existing series into a pie.
- A number that is not chart data ("we have forty desks", "the fortieth floor") is not a chart.

DIAGRAMS
- flow: steps or cause → effect (edge labels for causes); cycle: something that repeats; hub: a central idea and its parts (first node is the centre); timeline: dated events (year in `note`). 1–10 nodes, labels of 1–4 words taken from the speech.
- A diagram requires genuine structure: at least two actual stages, components, events, categories or relationships named by the presenter. A single factual claim or surprising capability is text even when its grammar suggests “A leads to B”.
- A process told step by step can start with its first step and grow with add_nodes. Never add a node that repeats one already there.
- If they restate or sum up a structure that is already on the board, do not draw a second copy: patch it (add_nodes, update_node, remove_node), or rebuild it with draw_diagram (full node list, or a different layout for the same nodes): draw_diagram replaces the diagram it matches in place. A node that was misheard is fixed with update_node.

TEXT
- Text is for structure the presenter explicitly creates: a heading or section label, an enumerated list, a stated takeaway, a concise headline claim, or a closing. Never transcribe ordinary narration and never turn a story into paragraphs on screen.
- `draw_text` replaces the one text tile on screen; text never accumulates into multiple tiles. Use heading for a title, paragraph for one short supporting thought, and bullet for each explicit item. Use `add_text_blocks` only for the first body beneath a heading or as the presenter continues the same explicit list. Patch corrections by block id with `update_text_block`; do not redraw the tile.
- `emphasis` contains 1–2 short, exact phrases copied from that block's text. The full concise block is visible and those phrases are underlined. Every block must have emphasis. Choose the fewest words that carry the point.
- A bare section cue such as “the scenario”, “the problem”, “the result” or “the takeaway” establishes that phrase as the heading. If the same finished sentence also states the content, draw one paragraph beneath it. Otherwise, when the board has that heading and NO paragraph, use `add_text_blocks` to add the next concise, relevant claim as the paragraph body. Never update or overwrite the heading with body prose.
- Once a section has a heading and paragraph, keep the heading and revise that paragraph by block id with `update_text_block` as the presenter develops the point. When the board lists any paragraph block, `add_text_blocks` is forbidden for further prose; never disguise prose as a bullet. For example, board `b1=The scenario | b2=Manual reporting` plus a new scenario detail means `update_text_block` targeting `b2`. Do not replace the card and do not accumulate paragraphs. Explicit enumerated lists (“first”, “second”, numbered items) use bullet blocks instead of a paragraph.
- Text must be extractive: use the presenter's own words and keep it concise. Never create text from unfinished words in "Being spoken now"; wait for the finished sentence.
- A closing such as "Thank you" is a text tile, usually heading "Thank you" and optional paragraph "Questions?" only when those words were said. Clear the old board only when the newest words also authorize `clear_board` with a quote.
- A single claim with one actor and one outcome is text, not a diagram: “customer service agents ended up doing the coding” should be a short heading/body card. Do not manufacture diagram nodes by splitting a sentence into its subject and predicate.

TECHNICAL DIAGRAMS (architecture, data flow, request paths, pipelines, infrastructure): draw_diagram with layout "flow" and an explicit `edges` list: one edge for EVERY connection the presenter describes ("the API talks to Postgres and Redis" is two edges out of the API), each with a 1–3 word label of what travels or happens ("writes", "publishes events", "token") when they say it. Node labels are the component names as said ("API gateway", "Orders service", "Postgres"). Draw the whole path in one diagram and grow it with add_nodes (with their edges) as more components are described.

ICONS ON NODES AND CHART POINTS. A node or point can carry a small picture, found in a library by exact name, so use exactly these words.
- `logo`: any node that is a NAMED product, technology or company: "Postgres", "Kafka", "Spark", "Snowflake", "Grafana", "Redis", "Docker", "React", "Node.js", "AWS S3", "Stripe", "GitHub" (also when the label adds a word: "Node.js API" is Node.js). A named product ALWAYS takes `logo`, never a generic icon, even if a generic icon would also fit. Never `logo` for a generic thing (a browser, an API gateway, a cache, a queue).
- `icon`: for everything else, ONE name from this list, the closest fit; leave it out when none fits (a wrong picture is worse than none): {ICON_NAMES}
- In technical diagrams give every node a logo or an icon: client → globe, monitor or smartphone; gateway or load balancer → network or router; service → server; queue → list-ordered; cache → database-zap; database → database; storage → hard-drive; users → users; auth → lock or key; event stream → workflow; monitoring → activity; container → container. Use the same icon for the same kind of thing throughout, so the diagram reads consistently.
- Diagrams of process steps, people or ideas get icons only when they add meaning. On charts use `logo` only when the bars or slices are companies or products (cloud providers, databases); no icons on other charts.

REMOVING AND CLEARING are the only irreversible actions, so remove, clear_board, remove_point, remove_node and remove_text_block each need a `quote`: the exact words, copied from the newest words, in which the presenter asks for it. If you cannot quote such words, they did not ask, so do not call it. A command in the newest words is always obeyed, even if a similar one was given a moment ago: a repeated command means it has not happened yet. Clearing is cheap, the board rebuilds itself from the next sentence.

Limits: 4 tiles, 3 annotations, 10 nodes per diagram, 8 points per chart, 8 blocks per text tile. If an op names an id or label that does not exist it is silently refused, so copy them exactly from the board."#;

pub fn tools() -> Value {
    let id = |what: &str| json!({"type": "string", "description": format!("id of the {what} on the board, e.g. e3")});
    let node_ref = json!({"type": "string", "description": "the node's id (n2) or its exact label"});
    let quote = json!({"type": "string", "description": "the exact words, copied from the newest words, in which the presenter asks for this"});
    let kind = json!({"type": "string", "enum": ["bar", "line", "pie", "stat"],
        "description": "bar: compare values, and the default whenever more numbers may follow; line: trend over 3+ times; pie: shares of a whole; stat: exactly one number, standing alone"});
    let icon_hint = json!({"type": "string", "description": "optional: ONE generic icon name from the list in the instructions (database, server, globe…); leave out if none fits"});
    let logo_hint = json!({"type": "string", "description": "optional: the product or company this IS (Postgres, Kafka, AWS S3); leave out if it is not a specific product"});
    let node = json!({"type": "object", "properties": {
        "label": {"type": "string", "description": "1–4 words"},
        "logo": logo_hint.clone(), "icon": icon_hint.clone(),
        "note": {"type": "string", "description": "optional: a year or ≤ 3-word detail"}}, "required": ["label"]});
    let edge = json!({"type": "object", "properties": {
        "from": {"type": "string", "description": "node label"}, "to": {"type": "string", "description": "node label"},
        "label": {"type": "string", "description": "optional ≤ 3 words, e.g. 'causes'"}}, "required": ["from", "to"]});
    let point = json!({"type": "object", "properties": {"label": {"type": "string"}, "value": {"type": "number"}, "logo": logo_hint.clone()}, "required": ["label", "value"]});
    let text_block = json!({"type": "object", "properties": {
        "kind": {"type": "string", "enum": ["heading", "paragraph", "bullet"]},
        "text": {"type": "string", "description": "concise words taken from finished speech"},
        "level": {"type": "integer", "minimum": 0, "maximum": 2, "description": "heading: 1 or 2; bullet: 0 or 1"},
        "emphasis": {"type": "array", "minItems": 1, "maxItems": 2, "items": {"type": "string"}, "description": "REQUIRED: short exact phrases within the visible text to underline"}
    }, "required": ["kind", "text", "emphasis"]});
    let tool = |name: &str, description: &str, properties: Value, required: Value| {
        json!({"type": "function", "function": {"name": name, "description": description,
            "parameters": {"type": "object", "properties": properties, "required": required}}})
    };
    json!([
        tool("no_action", "Change nothing. Use it when the newest words do not call for a change, and say why in a few words.",
            json!({"reason": {"type": "string"}}), json!(["reason"])),
        tool("show_photo", "Put a photo of a concrete thing on the board (searched in a photo library, or drawn if the library has none).",
            json!({"subject": {"type": "string", "description": "the exact thing to show, 1–4 words: 'owl', 'white rose', 'golden gate bridge'"},
                "mode": {"type": "string", "enum": ["add", "replace"], "description": "add (default): next to what is there. replace: swap the photo in focus for a variant of the same subject"}}),
            json!(["subject"])),
        tool("show_logo", "Put a company / product logo on the board, looked up by name in the logo library. If there is no such logo a plain name card is shown instead.",
            json!({"name": {"type": "string", "description": "just the brand: 'Google', 'Microsoft Azure', 'Baseten'"}, "mode": {"type": "string", "enum": ["add", "replace"], "description": "add (default): a new tile. replace: swap the logo/icon just shown for a better one (the newest words name it more exactly)"}}), json!(["name"])),
        tool("show_icon", "Put a generic icon (or a country flag) on the board, looked up by name and tags in the icon library. If nothing fits a plain card with the concept is shown.",
            json!({"concept": {"type": "string", "description": "the idea in 1–2 words: 'teamwork', 'database', 'security', 'flag of Canada'"},
                "alternatives": {"type": "array", "items": {"type": "string"}, "description": "2–4 other words such an icon could be named by, especially the plain object: teamwork → users, group, people"},
                "mode": {"type": "string", "enum": ["add", "replace"], "description": "add (default): a new tile. replace: swap the logo/icon just shown for a better one (the newest words name it more exactly)"}}),
            json!(["concept"])),
        tool("draw_chart", "Add a NEW chart built from numbers the presenter said. Use a stat for one large standalone amount such as a $100 million loss; use bars when values are compared or may grow into a series. Not for correcting or extending a chart already on the board.",
            json!({"kind": kind.clone(), "title": {"type": "string", "description": "≤ 6 words"}, "unit": {"type": "string", "description": "e.g. %, $, users, km"},
                "points": {"type": "array", "items": point, "description": "in the order spoken (chronological for time)"}}),
            json!(["kind", "points"])),
        tool("set_point", "CORRECT the value of one existing point: 'actually March was 80', 'make that 90', 'it's not 40, it's 45'. Leaves every other point alone.",
            json!({"id": id("chart"), "label": {"type": "string", "description": "the point's label as on the board (or what the presenter calls it)"}, "value": {"type": "number"}}),
            json!(["id", "label", "value"])),
        tool("add_point", "ADD a new point to a chart that is already on the board ('and in April we hit 95'). A stat becomes bars once it has two.",
            json!({"id": id("chart"), "label": {"type": "string"}, "value": {"type": "number"}, "logo": logo_hint}), json!(["id", "label", "value"])),
        tool("remove_point", "Take one point off a chart ('drop February', 'take March out'). Needs the presenter's words as `quote`.",
            json!({"id": id("chart"), "label": {"type": "string"}, "quote": quote.clone()}), json!(["id", "label", "quote"])),
        tool("set_chart", "Change a chart's kind, title or unit without touching its data ('show that as a line chart', 'call this chart monthly signups').",
            json!({"id": id("chart"), "kind": kind, "title": {"type": "string", "description": "≤ 6 words"}, "unit": {"type": "string"}}), json!(["id"])),
        tool("draw_text", "Replace the one text tile on screen with a heading, list, takeaway, concise headline claim or closing. Each concise block renders in full and its required emphasis phrases are underlined. Prefer this over a diagram for one actor and one outcome. Never transcribe ordinary narration or unfinished speech.",
            json!({"blocks": {"type": "array", "minItems": 1, "maxItems": 8, "items": text_block.clone()}}), json!(["blocks"])),
        tool("add_text_blocks", "Use only when the tile has NO paragraph: add its first paragraph, or append bullets to the same explicitly enumerated list. FORBIDDEN when any paragraph is on the tile; update that paragraph by block id. Prose is never a bullet.",
            json!({"id": id("text tile"), "blocks": {"type": "array", "minItems": 1, "items": text_block.clone()}}), json!(["id", "blocks"])),
        tool("update_text_block", "Revise one existing block in place. Use it for corrections and to update an EXISTING paragraph body beneath a persistent heading. Never replace a heading with body prose; if the tile has only a heading, add its first paragraph. Only include fields that changed.",
            json!({"id": id("text tile"), "block": {"type": "string", "description": "block id from the board, e.g. b4"}, "text": {"type": "string"}, "level": {"type": "integer"}, "emphasis": {"type": "array", "items": {"type": "string"}}}), json!(["id", "block"])),
        tool("remove_text_block", "Remove one item from a text tile. Needs the presenter's words as `quote`.",
            json!({"id": id("text tile"), "block": {"type": "string"}, "quote": quote.clone()}), json!(["id", "block", "quote"])),
        tool("draw_diagram", "Add a NEW diagram, or rebuild the one it matches in place (same steps, or a different layout for them).",
            json!({"layout": {"type": "string", "enum": ["flow", "cycle", "hub", "timeline"],
                    "description": "flow: steps / cause→effect left to right; cycle: steps that repeat; hub: first node is the centre, others are its parts; timeline: dated events (put the date in note)"},
                "title": {"type": "string", "description": "≤ 6 words"},
                "nodes": {"type": "array", "items": node.clone(), "description": "1–10 nodes in order"},
                "edges": {"type": "array", "items": edge.clone(), "description": "one per connection described (fan-out, fan-in, labelled); omit only for a plain chain (flow/cycle/timeline) or spokes (hub)"}}),
            json!(["layout", "nodes"])),
        tool("add_nodes", "Add nodes (and optional edges) to a diagram already on the board: the presenter keeps describing the same structure.",
            json!({"id": id("diagram"), "nodes": {"type": "array", "items": node}, "edges": {"type": "array", "items": edge.clone()}}), json!(["id", "nodes"])),
        tool("update_node", "Rename a diagram node or change its note ('call the second step compile', a misheard label).",
            json!({"id": id("diagram"), "node": node_ref.clone(), "label": {"type": "string"}, "note": {"type": "string"}}), json!(["id", "node"])),
        tool("remove_node", "Take one node out of a diagram ('skip the test step'). Needs the presenter's words as `quote`.",
            json!({"id": id("diagram"), "node": node_ref.clone(), "quote": quote.clone()}), json!(["id", "node", "quote"])),
        tool("add_edge", "Link two nodes of a diagram.",
            json!({"id": id("diagram"), "from": node_ref.clone(), "to": node_ref.clone(), "label": {"type": "string"}}), json!(["id", "from", "to"])),
        tool("remove_edge", "Remove the link between two nodes of a diagram.",
            json!({"id": id("diagram"), "from": node_ref.clone(), "to": node_ref}), json!(["id", "from", "to"])),
        tool("focus", "Make one tile the focus.", json!({"id": id("tile")}), json!(["id"])),
        tool("remove", "Take one whole tile off the board ('take the eagle away', 'get rid of the chart'). Needs the presenter's words as `quote`.",
            json!({"id": id("tile"), "quote": quote.clone()}), json!(["id", "quote"])),
        tool("arrange", "Re-lay out the board.",
            json!({"layout": {"type": "string", "enum": ["auto", "hero", "compare", "grid"],
                "description": "hero: focus big + others small; compare: side by side; grid: all equal; auto: by count"}}), json!(["layout"])),
        tool("annotate", "Draw attention: highlight/frame one tile, or an arrow between two.",
            json!({"kind": {"type": "string", "enum": ["highlight", "frame", "arrow"]},
                "targets": {"type": "array", "items": {"type": "string"}, "description": "tile ids (2 for arrow)"},
                "label": {"type": "string", "description": "optional, ≤ 5 words"}}), json!(["kind", "targets"])),
        tool("clear_annotations", "Remove all annotations.", json!({}), json!([])),
        tool("clear_board", "Clear everything (a new section of the talk). Needs the presenter's words as `quote`.", json!({"quote": quote}), json!(["quote"])),
    ])
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Model,
    Rules,
}

/// One finished sentence of the talk and when it was said (seconds since the talk started).
#[derive(Debug, Clone, PartialEq)]
pub struct Sentence {
    pub at_s: u64,
    pub text: String,
}

/// One thing already done to the board, for the "recent changes" list.
#[derive(Debug, Clone, PartialEq)]
pub struct Change {
    pub at_s: u64,
    pub what: String,
}

/// Everything one call sees.
pub struct AgentInput<'a> {
    pub scene: &'a Scene,
    /// Every finished sentence, oldest first.
    pub transcript: &'a [Sentence],
    /// Index into `transcript` of the first sentence the agent has not seen in an earlier call.
    pub new_from: usize,
    /// The phrase being spoken right now (may be unfinished).
    pub speaking_now: &'a str,
    pub changes: &'a [Change],
    pub now_s: u64,
}

impl AgentInput<'_> {
    /// The words the agent may act on (and quote): what is new since its last call, plus the phrase in progress.
    pub fn newest_text(&self) -> String {
        let new: Vec<&str> = self.transcript.iter().skip(self.new_from).map(|s| s.text.as_str()).chain(std::iter::once(self.speaking_now)).filter(|t| !t.trim().is_empty()).collect();
        new.join(" ")
    }
    fn transcript_text(&self) -> String {
        self.transcript.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(" ")
    }
}

/// What a call produced.
#[derive(Debug, Clone)]
pub struct Proposal {
    pub ops: Vec<Op>,
    pub source: Source,
    /// Ops the guards refused, each with the reason (logged by the pipeline).
    pub dropped: Vec<String>,
    /// Why the model call failed, when it did and the rules answered instead.
    pub error: Option<String>,
    pub transport: Transport,
    /// Time from sending `response.create` until the first WebSocket event. HTTP does not expose this split.
    pub first_event_ms: Option<u64>,
    /// The service tier OpenAI says actually served the request (`priority` means Fast mode).
    pub service_tier: Option<String>,
}

fn clock(s: u64) -> String {
    format!("{}:{:02}", s / 60, s % 60)
}

/// The board as the model sees it: ids, contents, focus, layout, limits.
pub fn board_json(scene: &Scene) -> Value {
    let tiles: Vec<Value> = scene
        .elements
        .iter()
        .map(|e| match e.kind {
            ElementKind::Image => json!({"id": e.id, "photo": e.caption, "focus": e.focus}),
            ElementKind::Logo => json!({"id": e.id, "logo": e.caption, "name_card_only": e.image_id.is_empty(), "focus": e.focus}),
            ElementKind::Diagram => {
                let d = e.diagram.as_ref();
                let label = |id: &str| d.and_then(|d| d.nodes.iter().find(|n| n.id == id)).map(|n| n.label.clone()).unwrap_or_else(|| id.to_string());
                json!({"id": e.id, "diagram": d.map(|d| d.layout), "title": d.and_then(|d| d.title.clone()), "focus": e.focus,
                    "nodes": d.map(|d| d.nodes.iter().map(|n| json!({"id": n.id, "label": n.label, "note": n.note})).collect::<Vec<_>>()).unwrap_or_default(),
                    "edges": d.map(|d| d.edges.iter().map(|x| json!([label(&x.from), label(&x.to), x.label])).collect::<Vec<_>>()).unwrap_or_default()})
            }
            ElementKind::Chart => {
                let c = e.chart.as_ref();
                json!({"id": e.id, "chart": c.map(|c| c.kind), "title": c.and_then(|c| c.title.clone()), "unit": c.and_then(|c| c.unit.clone()), "focus": e.focus,
                    "points": c.map(|c| c.points.iter().map(|p| json!([p.label, p.value])).collect::<Vec<_>>()).unwrap_or_default()})
            }
            ElementKind::Text => {
                let t = e.text.as_ref();
                json!({"id": e.id, "text": t.map(|t| t.blocks.iter().map(|b| json!({"id": b.id, "kind": b.kind, "text": b.text, "level": b.level, "emphasis": b.emphasis})).collect::<Vec<_>>()).unwrap_or_default(), "focus": e.focus})
            }
        })
        .collect();
    let notes: Vec<Value> = scene.annotations.iter().map(|a| json!({"kind": a.kind, "targets": a.targets, "label": a.label})).collect();
    json!({"tiles": tiles, "layout": scene.layout, "annotations": notes, "limits": {"tiles": ls_canvas::MAX_ELEMENTS, "annotations": ls_canvas::MAX_ANNOTATIONS, "nodes": ls_canvas::MAX_NODES, "points": ls_canvas::MAX_POINTS, "text_blocks": ls_canvas::MAX_TEXT_BLOCKS}})
}

/// The user message. Order matters for provider prompt caching: the transcript only ever grows at its end, so
/// everything before "Board" is a stable prefix across calls; what changes every call comes last.
pub fn user_message(input: &AgentInput) -> String {
    // Only what was said BEFORE the new words: those are listed once, under "Newest words". Showing them in both
    // places made "earlier speech" ambiguous (the model called the newest sentence "already spoken earlier").
    let mut lines: Vec<String> = input.transcript.iter().take(input.new_from).map(|s| format!("[{}] {}", clock(s.at_s), s.text.trim())).collect();
    let mut omitted = 0;
    let mut size: usize = lines.iter().map(|l| l.len() + 1).sum();
    while size > TRANSCRIPT_CHARS && lines.len() > 1 {
        size -= lines.remove(0).len() + 1;
        omitted += 1;
    }
    let mut out = String::from("## Transcript before the newest words (oldest first)\n");
    if omitted > 0 {
        out += &format!("({omitted} earlier sentences omitted)\n");
    }
    out += &if lines.is_empty() { "(nothing said before the newest words)\n".to_string() } else { lines.join("\n") + "\n" };
    out += &format!("\n## Board\n{}\n", board_json(input.scene));
    out += "\n## Recent changes (newest last)\n";
    out += &if input.changes.is_empty() {
        "(none yet)\n".to_string()
    } else {
        input.changes.iter().map(|c| format!("[{}] {}", clock(c.at_s), c.what)).collect::<Vec<_>>().join("\n") + "\n"
    };
    out += &format!("\n## Newest words: act on these (now {})\n", clock(input.now_s));
    let new: Vec<String> = input.transcript.iter().skip(input.new_from).map(|s| format!("[{}] {}", clock(s.at_s), s.text.trim())).collect();
    out += &if new.is_empty() { "Said since your last call: (nothing new)\n".to_string() } else { format!("Said since your last call:\n{}\n", new.join("\n")) };
    out += &if input.speaking_now.trim().is_empty() { "Being spoken now: (silence)\n".to_string() } else { format!("Being spoken now (may be unfinished): {}\n", input.speaking_now.trim()) };
    out
}

/// Once a Responses chain exists, its earlier transcript and instructions are already in model state. Send only
/// the authoritative current board, recent outcomes, and genuinely new speech on later turns.
pub fn incremental_user_message(input: &AgentInput) -> String {
    let mut out = format!("## Board now\n{}\n", board_json(input.scene));
    out += "\n## Recent changes (newest last)\n";
    out += &if input.changes.is_empty() {
        "(none yet)\n".to_string()
    } else {
        input.changes.iter().map(|c| format!("[{}] {}", clock(c.at_s), c.what)).collect::<Vec<_>>().join("\n") + "\n"
    };
    out += &format!("\n## Newest words: act on these (now {})\n", clock(input.now_s));
    let new: Vec<String> = input.transcript.iter().skip(input.new_from).map(|s| format!("[{}] {}", clock(s.at_s), s.text.trim())).collect();
    out += &if new.is_empty() { "Said since your last call: (nothing new)\n".to_string() } else { format!("Said since your last call:\n{}\n", new.join("\n")) };
    out += &if input.speaking_now.trim().is_empty() { "Being spoken now: (silence)\n".to_string() } else { format!("Being spoken now (may be unfinished): {}\n", input.speaking_now.trim()) };
    out
}

type ResponsesSocket = WebSocketStream<MaybeTlsStream<TcpStream>>;

struct WsState {
    socket: Option<ResponsesSocket>,
    previous_response_id: Option<String>,
    pending_call_ids: Vec<String>,
    pending_output: Option<String>,
    generated_turn: bool,
}

impl Default for WsState {
    fn default() -> Self {
        Self { socket: None, previous_response_id: None, pending_call_ids: vec![], pending_output: None, generated_turn: false }
    }
}

struct WsReply {
    response: Value,
    first_event_ms: u64,
}

#[derive(Clone)]
pub struct CanvasAgent {
    http: reqwest::Client,
    /// OpenRouter key.
    api_key: Option<String>,
    /// OpenAI key; when present it wins (`CANVAS_PROVIDER=openrouter` overrides that).
    openai_key: Option<String>,
    pub model: String,
    /// First attempt; a timeout or a 429/5xx gets one more, shorter, attempt (`CANVAS_TIMEOUT_MS`, default 6000).
    timeout: Duration,
    /// OpenAI processing tier. Standard is the application default; Fast remains an explicit experiment.
    service_tier: String,
    /// One live Responses connection and response chain. `CanvasAgent` clones share it so the pipeline can make
    /// the call in a task, apply the returned ops, then report those outcomes to the next turn.
    ws: Arc<Mutex<WsState>>,
}

impl CanvasAgent {
    pub fn new(http: reqwest::Client, api_key: Option<String>, model: Option<String>) -> Self {
        Self {
            http,
            api_key: api_key.filter(|k| !k.trim().is_empty()),
            openai_key: None,
            model: model.unwrap_or_else(|| DEFAULT_MODEL.into()),
            timeout: Duration::from_millis(std::env::var("CANVAS_TIMEOUT_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(6000)),
            service_tier: std::env::var("CANVAS_SERVICE_TIER").unwrap_or_else(|_| "default".into()),
            ws: Arc::new(Mutex::new(WsState::default())),
        }
    }

    /// Use OpenAI's own API with this key (when it is a non-empty string).
    pub fn with_openai(mut self, key: Option<String>) -> Self {
        self.openai_key = key.filter(|k| !k.trim().is_empty());
        self
    }

    /// The backend a call goes to, or `None` when no key is set (offline rules only).
    pub fn provider(&self) -> Option<Provider> {
        let forced = std::env::var("CANVAS_PROVIDER").map(|v| v.to_lowercase()).unwrap_or_default();
        match (self.openai_key.is_some(), self.api_key.is_some()) {
            (true, _) if forced != "openrouter" => Some(Provider::OpenAi),
            (_, true) => Some(Provider::OpenRouter),
            (true, false) => Some(Provider::OpenAi),
            _ => None,
        }
    }

    pub fn has_remote(&self) -> bool {
        self.provider().is_some()
    }

    /// WebSocket mode remains an explicit rollout switch until it matches the established HTTP probe baseline.
    /// OpenRouter always uses Chat Completions.
    pub fn websocket_enabled(&self) -> bool {
        self.provider() == Some(Provider::OpenAi)
            && matches!(std::env::var("CANVAS_TRANSPORT").unwrap_or_default().to_lowercase().as_str(), "ws" | "websocket")
    }

    /// Calls per minute the backend allows for this account (a new OpenRouter account is capped at 20/min for
    /// Luna; the OpenAI key measured 500/min and 500k tokens/min).
    pub fn default_rpm(&self) -> u32 {
        if self.provider() == Some(Provider::OpenAi) { 500 } else { 18 }
    }

    /// The model id as the backend spells it.
    pub fn wire_model(&self) -> String {
        match self.provider() {
            Some(Provider::OpenAi) => self.model.strip_prefix("openai/").unwrap_or(&self.model).to_string(),
            _ if self.model.contains('/') => self.model.clone(),
            _ => format!("openai/{}", self.model),
        }
    }

    /// Ops for the current board given what was just said. Never fails: a model error falls back to the offline cue rules.
    pub async fn propose(&self, input: &AgentInput<'_>) -> Proposal {
        let newest = input.newest_text();
        let mut error = None;
        if self.websocket_enabled() {
            match tokio::time::timeout(self.timeout, self.call_responses_ws(input)).await {
                Ok(Ok(reply)) => {
                    let (ops, dropped) = ground(parse_response_tool_calls(&reply.response), input.scene, &input.transcript_text(), &newest, input.new_from < input.transcript.len());
                    return Proposal {
                        ops,
                        source: Source::Model,
                        dropped,
                        error: None,
                        transport: Transport::ResponsesWebSocket,
                        first_event_ms: Some(reply.first_event_ms),
                        service_tier: reply.response["service_tier"].as_str().map(String::from),
                    };
                }
                Ok(Err(e)) => {
                    eprintln!("canvas agent websocket: {e:#}; retrying over HTTP");
                    error = Some(format!("websocket failed: {e:#}"));
                    self.reset_websocket().await;
                }
                Err(_) => {
                    eprintln!("canvas agent websocket: timed out after {:?}; retrying over HTTP", self.timeout);
                    error = Some(format!("websocket timed out after {:?}", self.timeout));
                    self.reset_websocket().await;
                }
            }
        }
        if self.has_remote() {
            for attempt in 0..2 {
                let budget = if attempt == 0 { self.timeout } else { self.timeout * 2 / 3 };
                match tokio::time::timeout(budget, self.call_raw(input)).await {
                    Ok(Ok(text)) => {
                        let (ops, dropped) = ground(parse_tool_calls(&text), input.scene, &input.transcript_text(), &newest, input.new_from < input.transcript.len());
                        let service_tier = serde_json::from_str::<Value>(&text)
                            .ok()
                            .and_then(|response| response["service_tier"].as_str().map(String::from));
                        return Proposal { ops, source: Source::Model, dropped, error, transport: Transport::ChatHttp, first_event_ms: None, service_tier };
                    }
                    Ok(Err(e)) => {
                        let retry = attempt == 0 && (e.to_string().starts_with("HTTP 429") || e.to_string().starts_with("HTTP 5"));
                        eprintln!("canvas agent: {e:#}");
                        let http_error = format!("{e:#}");
                        error = Some(match error { Some(previous) => format!("{previous}; HTTP failed: {http_error}"), None => http_error });
                        if !retry {
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(600)).await;
                    }
                    Err(_) => {
                        eprintln!("canvas agent: timed out after {budget:?}");
                        error = Some(format!("timed out after {budget:?}"));
                    }
                }
            }
        }
        Proposal { ops: rule_ops(&newest, input.scene), source: Source::Rules, dropped: vec![], error, transport: Transport::Rules, first_event_ms: None, service_tier: None }
    }

    /// Establish the Responses socket and prepare the stable instructions and tools without generating output.
    /// This is called during app startup; `propose` also calls it lazily if startup warmup did not complete.
    pub async fn warm_up(&self) -> anyhow::Result<Option<u64>> {
        if !self.websocket_enabled() {
            return Ok(None);
        }
        let started = Instant::now();
        tokio::time::timeout(self.timeout, async {
            let mut state = self.ws.lock().await;
            self.ensure_ws_warm(&mut state).await
        })
        .await
        .map_err(|_| anyhow::anyhow!("Luna WebSocket warmup timed out after {:?}", self.timeout))??;
        Ok(Some(started.elapsed().as_millis() as u64))
    }

    /// Feed the real canvas result back as the output for every tool call in the last response. The next turn
    /// also carries the full current board, so this concise receipt is enough to keep the chain honest.
    pub async fn acknowledge(&self, applied: &[String], refused: &[String], scene: &Scene) {
        if !self.websocket_enabled() {
            return;
        }
        let mut state = self.ws.lock().await;
        if !state.pending_call_ids.is_empty() {
            state.pending_output = Some(json!({"applied": applied, "refused": refused, "board": board_json(scene)}).to_string());
        }
    }

    /// Drop conversation state between unrelated probe cases or after a connection failure.
    pub async fn reset_websocket(&self) {
        let mut state = self.ws.lock().await;
        if let Some(mut socket) = state.socket.take() {
            let _ = socket.close(None).await;
        }
        *state = WsState::default();
    }

    /// The chat-completions request. The two backends disagree about the model's parameters:
    /// - **OpenAI** (`gpt-5.6-luna`): `max_completion_tokens` (not `max_tokens`); `reasoning_effort` is a top-level
    ///   string and function tools are only accepted with `"none"` (`minimal` is not a value for this model; the
    ///   alternative is the Responses API); no `reasoning` object and no `provider` block (both are rejected as
    ///   unknown parameters); `temperature` is fine with reasoning off.
    /// - **OpenRouter**: `max_tokens`, a `reasoning: {effort}` object, and `provider: {sort: latency}` routing.
    pub fn request_body(&self, input: &AgentInput<'_>) -> Value {
        let model = self.wire_model();
        let mut body = json!({
            "model": model,
            "messages": [{"role": "system", "content": system_prompt()}, {"role": "user", "content": user_message(input)}],
            "tools": tools(),
            "tool_choice": "required",
            "temperature": 0,
        });
        match self.provider() {
            Some(Provider::OpenAi) => {
                body["max_completion_tokens"] = json!(700);
                // Standard was faster and more consistent than Fast mode in the chained Luna benchmark.
                // The response tier is logged so an explicit override remains measurable.
                body["service_tier"] = json!(self.service_tier);
                if model.starts_with("gpt-5") {
                    body["reasoning_effort"] = json!("none");
                }
            }
            _ => {
                body["max_tokens"] = json!(700);
                body["provider"] = json!({"sort": "latency"});
                // Thinking models (Gemini 2.5 Flash, …) are ~2× slower with reasoning on; this call needs none.
                if model.starts_with("openai/gpt-5") || model.contains("gpt-oss") {
                    body["reasoning"] = json!({"effort": "minimal"});
                } else if !model.starts_with("anthropic/") {
                    body["reasoning"] = json!({"enabled": false});
                }
            }
        }
        body
    }

    /// The provider's raw JSON reply: no timeout, no parsing. For latency and token probes.
    pub async fn call_raw(&self, input: &AgentInput<'_>) -> anyhow::Result<String> {
        let (url, key) = match self.provider() {
            Some(Provider::OpenAi) => (format!("{}/v1/chat/completions", openai_base()), self.openai_key.as_deref()),
            _ => (format!("{}/api/v1/chat/completions", base()), self.api_key.as_deref()),
        };
        let resp = self.http.post(url).bearer_auth(key.unwrap_or_default()).json(&self.request_body(input)).send().await?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("HTTP {status}: {}", &text[..text.len().min(300)]);
        }
        Ok(text)
    }

    async fn connect_responses(&self) -> anyhow::Result<ResponsesSocket> {
        let mut url = format!("{}/v1/responses", openai_base().trim_end_matches('/'));
        if let Some(rest) = url.strip_prefix("https://") {
            url = format!("wss://{rest}");
        } else if let Some(rest) = url.strip_prefix("http://") {
            url = format!("ws://{rest}");
        }
        let mut request = url.into_client_request()?;
        let key = self.openai_key.as_deref().ok_or_else(|| anyhow::anyhow!("OPENAI_API_KEY is not set"))?;
        request.headers_mut().insert(header::AUTHORIZATION, HeaderValue::from_str(&format!("Bearer {key}"))?);
        request.headers_mut().insert("OpenAI-Beta", HeaderValue::from_static("responses_websockets=2026-02-06"));
        let (socket, _) = connect_async(request).await?;
        Ok(socket)
    }

    async fn ensure_ws_warm(&self, state: &mut WsState) -> anyhow::Result<()> {
        if state.socket.is_some() && state.previous_response_id.is_some() {
            return Ok(());
        }
        let mut socket = self.connect_responses().await?;
        let request = json!({
            "type": "response.create",
            "stream_id": "live-slides",
            "model": self.wire_model(),
            "store": false,
            "generate": false,
            "input": [],
            "instructions": system_prompt(),
            "tools": response_tools(),
            "reasoning": {"effort": "none"},
            "temperature": 0,
            "service_tier": self.service_tier,
            "max_output_tokens": 700
        });
        socket.send(Message::Text(request.to_string().into())).await?;
        let reply = wait_for_response(&mut socket, "live-slides").await?;
        let id = reply.response["id"].as_str().ok_or_else(|| anyhow::anyhow!("warmup response had no id: {}", reply.response))?;
        state.socket = Some(socket);
        state.previous_response_id = Some(id.to_string());
        state.pending_call_ids.clear();
        state.pending_output = None;
        state.generated_turn = false;
        Ok(())
    }

    async fn call_responses_ws(&self, input: &AgentInput<'_>) -> anyhow::Result<WsReply> {
        let mut state = self.ws.lock().await;
        self.ensure_ws_warm(&mut state).await?;

        let previous = state.previous_response_id.clone().ok_or_else(|| anyhow::anyhow!("WebSocket chain was not warmed"))?;
        let mut items = vec![];
        if !state.pending_call_ids.is_empty() {
            let output = state.pending_output.take().unwrap_or_else(|| json!({"status": "accepted for application"}).to_string());
            for call_id in state.pending_call_ids.drain(..) {
                items.push(json!({"type": "function_call_output", "call_id": call_id, "output": output}));
            }
        }
        let message = if state.generated_turn { incremental_user_message(input) } else { user_message(input) };
        items.push(json!({"type": "message", "role": "user", "content": [{"type": "input_text", "text": message}]}));
        let request = json!({
            "type": "response.create",
            "stream_id": "live-slides",
            "model": self.wire_model(),
            "store": false,
            "previous_response_id": previous,
            "instructions": system_prompt(),
            "input": items,
            "tools": response_tools(),
            "tool_choice": "required",
            "parallel_tool_calls": true,
            "reasoning": {"effort": "none"},
            "temperature": 0,
            "service_tier": self.service_tier,
            "max_output_tokens": 700
        });
        let socket = state.socket.as_mut().ok_or_else(|| anyhow::anyhow!("WebSocket disconnected before send"))?;
        socket.send(Message::Text(request.to_string().into())).await?;
        let reply = wait_for_response(socket, "live-slides").await?;
        let id = reply.response["id"].as_str().ok_or_else(|| anyhow::anyhow!("completed response had no id: {}", reply.response))?;
        state.previous_response_id = Some(id.to_string());
        state.pending_call_ids = response_call_ids(&reply.response);
        state.generated_turn = true;
        Ok(reply)
    }
}

/// Responses uses the function fields directly on each tool, while Chat Completions nests them under
/// `function`. Keep one source of truth for the schemas and convert only at the transport boundary.
pub fn response_tools() -> Value {
    Value::Array(
        tools()
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|tool| {
                let function = tool.get("function")?.as_object()?;
                Some(json!({
                    "type": "function",
                    "name": function.get("name")?,
                    "description": function.get("description")?,
                    "parameters": function.get("parameters")?
                }))
            })
            .collect(),
    )
}

async fn wait_for_response(socket: &mut ResponsesSocket, stream_id: &str) -> anyhow::Result<WsReply> {
    let started = Instant::now();
    let mut first_event_ms = None;
    while let Some(message) = socket.next().await {
        let message = message?;
        let text = match message {
            Message::Text(text) => text.to_string(),
            Message::Binary(bytes) => String::from_utf8(bytes.to_vec())?,
            Message::Ping(bytes) => {
                socket.send(Message::Pong(bytes)).await?;
                continue;
            }
            Message::Close(frame) => anyhow::bail!("Responses WebSocket closed before completion: {frame:?}"),
            _ => continue,
        };
        first_event_ms.get_or_insert(started.elapsed().as_millis() as u64);
        let raw: Value = serde_json::from_str(&text)?;
        // The SDK iterator wraps raw server events in `{type:"message", message:...}`. Accept either form so
        // local mocks can use the SDK-visible shape while production uses the wire event directly.
        let event = if raw["type"] == "message" { &raw["message"] } else { &raw };
        if event.get("stream_id").and_then(Value::as_str).is_some_and(|id| id != stream_id) {
            continue;
        }
        match event["type"].as_str().unwrap_or_default() {
            "response.completed" => {
                return Ok(WsReply { response: event["response"].clone(), first_event_ms: first_event_ms.unwrap_or_default() });
            }
            "response.failed" | "response.incomplete" | "error" => anyhow::bail!("Responses event: {event}"),
            _ => {}
        }
    }
    anyhow::bail!("Responses WebSocket ended before response.completed")
}

fn response_call_ids(response: &Value) -> Vec<String> {
    response["output"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|item| item["type"] == "function_call")
        .filter_map(|item| item["call_id"].as_str().map(String::from))
        .collect()
}

/// Responses `output[*].{name,arguments}` → the same parser used by Chat Completions.
pub fn parse_response_tool_calls(response: &Value) -> Vec<Op> {
    let calls: Vec<Value> = response["output"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|item| item["type"] == "function_call")
        .map(|item| json!({"function": {"name": item["name"], "arguments": item["arguments"]}}))
        .collect();
    parse_tool_calls(&json!({"choices": [{"message": {"tool_calls": calls}}]}).to_string())
}

fn norm_words(s: &str) -> Vec<String> {
    s.to_lowercase().replace('’', "'").split(|c: char| !(c.is_alphanumeric() || c == '\'')).filter(|w| !w.is_empty()).map(String::from).collect()
}

/// The quoted words really are in the newest words: every word of the quote, in order (a model that drops or
/// swaps a filler word still passes; one that quotes something never said does not).
pub fn quote_ok(quote: &str, newest: &str) -> bool {
    let (q, hay) = (norm_words(quote), norm_words(newest));
    if q.is_empty() {
        return false;
    }
    let mut it = hay.iter();
    q.iter().all(|w| it.any(|h| h == w))
}

/// Code-level guards on model output (prompt rules alone were not reliable, 09-19 probes):
/// - remove / clear_board / remove_point / remove_node only with a `quote` that is in the newest words. This
///   replaces the phrase lists (`take the eagle away` was not on one): the model judges the language, the code
///   only checks that the presenter's words were really there;
/// - chart values must be numbers the presenter said (or already on the board), which drops invented remainders
///   like "Not stoned: 40". A number spoken with a scale word grounds the bare value too ("eighteen million" → 18);
/// - visible text is created or edited only after a new sentence has finished;
/// - empty photo requests are dropped.
/// Returns the surviving ops and a reason for every op or point that was refused.
pub fn ground(ops: Vec<Op>, scene: &Scene, transcript: &str, newest: &str, has_finished_new: bool) -> (Vec<Op>, Vec<String>) {
    let mut said = spoken_numbers(&format!("{transcript} {newest}"));
    for e in &scene.elements {
        if let Some(c) = &e.chart {
            said.extend(c.points.iter().map(|p| p.value));
        }
    }
    let close = |a: f64, b: f64| (a - b).abs() <= 0.005 * a.abs().max(b.abs()) + 1e-9;
    let grounded = |v: f64| said.iter().any(|s| [1.0, 1e3, 1e6, 1e9].iter().any(|k| close(*s, v * k)));
    let mut dropped = vec![];
    let mut out = vec![];
    for op in ops {
        if let Some(q) = op.quote() {
            match q {
                Some(q) if quote_ok(q, newest) => {}
                Some(q) => {
                    dropped.push(format!("{}: quote {q:?} is not in the newest words", ls_canvas::op_name(&op)));
                    continue;
                }
                None => {
                    dropped.push(format!("{}: no quote", ls_canvas::op_name(&op)));
                    continue;
                }
            }
        }
        match op {
            Op::DrawText { .. } | Op::AddTextBlocks { .. } | Op::UpdateTextBlock { .. } if !has_finished_new => {
                dropped.push(format!("{}: text waits for finished speech", ls_canvas::op_name(&op)));
            }
            Op::DrawChart { kind, title, unit, points } => {
                let (keep, lost): (Vec<Point>, Vec<Point>) = points.into_iter().partition(|p| grounded(p.value));
                dropped.extend(lost.iter().map(|p| format!("draw_chart: {} {} was never said", p.label, p.value)));
                if !keep.is_empty() {
                    out.push(Op::DrawChart { kind, title, unit, points: keep });
                }
            }
            Op::SetPoint { ref label, value, .. } | Op::AddPoint { ref label, value, .. } if !grounded(value) => {
                dropped.push(format!("{}: {label} {value} was never said", ls_canvas::op_name(&op)));
            }
            Op::UpdateChart { id, kind, title, points } => {
                let points: Vec<Point> = points.into_iter().filter(|p| grounded(p.value)).collect();
                if !points.is_empty() || title.is_some() {
                    out.push(Op::UpdateChart { id, kind, title, points });
                }
            }
            Op::ShowPhoto { ref subject, .. } if subject.trim().is_empty() => dropped.push("show_photo: empty subject".into()),
            Op::ShowLogo { ref name, .. } if name.trim().is_empty() => dropped.push("show_logo: empty name".into()),
            Op::ShowIcon { ref concept, .. } if concept.trim().is_empty() => dropped.push("show_icon: empty concept".into()),
            other => out.push(other),
        }
    }
    (out, dropped)
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
                    if cur != 0.0 {
                        out.push(cur); // "eighteen million" also says 18: a chart kept in millions holds 18
                    }
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
                Value::String(s) if s.trim().is_empty() => json!({}),
                Value::String(s) => serde_json::from_str(s).ok()?,
                other => other.clone(),
            };
            let s = |k: &str| args[k].as_str().map(String::from);
            // Labels are text, but a model may send a year as a number.
            let text = |k: &str| args[k].as_str().map(String::from).or_else(|| args[k].as_i64().map(|n| n.to_string()));
            let opt = |k: &str| s(k).filter(|t| !t.trim().is_empty());
            Some(match name {
                "no_action" => Op::NoAction { reason: s("reason").unwrap_or_default() },
                "show_photo" => Op::ShowPhoto { subject: s("subject")?, replace: s("mode").as_deref() == Some("replace") },
                "show_logo" => Op::ShowLogo { name: s("name")?, replace: s("mode").as_deref() == Some("replace") },
                "show_icon" => Op::ShowIcon {
                    concept: s("concept")?,
                    alternatives: args["alternatives"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default(),
                    replace: s("mode").as_deref() == Some("replace"),
                },
                "focus" => Op::Focus { id: s("id")? },
                "remove" => Op::Remove { id: s("id")?, quote: opt("quote") },
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
                    label: opt("label"),
                },
                "clear_annotations" => Op::ClearAnnotations,
                "clear_board" => Op::ClearBoard { quote: opt("quote") },
                "draw_diagram" => Op::DrawDiagram {
                    layout: match s("layout")?.as_str() {
                        "flow" => DiagramLayout::Flow,
                        "cycle" => DiagramLayout::Cycle,
                        "hub" => DiagramLayout::Hub,
                        "timeline" => DiagramLayout::Timeline,
                        _ => return None,
                    },
                    title: opt("title"),
                    nodes: nodes(&args["nodes"]),
                    edges: edges(&args["edges"]),
                },
                "add_nodes" => Op::AddNodes { id: s("id")?, nodes: nodes(&args["nodes"]), edges: edges(&args["edges"]) },
                "update_node" => Op::UpdateNode { id: s("id")?, node: text("node")?, label: opt("label"), note: s("note") },
                "remove_node" => Op::RemoveNode { id: s("id")?, node: text("node")?, quote: opt("quote") },
                "add_edge" => Op::AddEdge { id: s("id")?, from: text("from")?, to: text("to")?, label: opt("label") },
                "remove_edge" => Op::RemoveEdge { id: s("id")?, from: text("from")?, to: text("to")? },
                "draw_chart" => Op::DrawChart { kind: chart_kind(&s("kind")?)?, title: opt("title"), unit: opt("unit"), points: points(&args["points"]) },
                "set_point" => Op::SetPoint { id: s("id")?, label: text("label")?, value: number(&args["value"])? },
                "add_point" => Op::AddPoint { id: s("id")?, label: text("label")?, value: number(&args["value"])?, icon: opt("icon"), logo: opt("logo") },
                "remove_point" => Op::RemovePoint { id: s("id")?, label: text("label")?, quote: opt("quote") },
                "set_chart" => Op::SetChart { id: s("id")?, kind: s("kind").and_then(|k| chart_kind(&k)), title: opt("title"), unit: s("unit") },
                "draw_text" => Op::DrawText { blocks: text_blocks(&args["blocks"]) },
                "add_text_blocks" => Op::AddTextBlocks { id: s("id")?, blocks: text_blocks(&args["blocks"]) },
                "update_text_block" => Op::UpdateTextBlock {
                    id: s("id")?, block: s("block")?, text: opt("text"),
                    emphasis: args.get("emphasis").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()),
                    level: args.get("level").and_then(|v| v.as_u64()).map(|n| n.min(255) as u8),
                },
                "remove_text_block" => Op::RemoveTextBlock { id: s("id")?, block: s("block")?, quote: opt("quote") },
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

fn text_blocks(v: &Value) -> Vec<TextBlockSpec> {
    v.as_array().map(|a| a.iter().filter_map(|b| {
        let kind = match b["kind"].as_str()? {
            "heading" => TextBlockKind::Heading,
            "paragraph" => TextBlockKind::Paragraph,
            "bullet" => TextBlockKind::Bullet,
            _ => return None,
        };
        Some(TextBlockSpec {
            kind,
            text: b["text"].as_str()?.to_string(),
            level: b["level"].as_u64().unwrap_or(if kind == TextBlockKind::Heading { 1 } else { 0 }).min(255) as u8,
            emphasis: b["emphasis"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default(),
        })
    }).collect()).unwrap_or_default()
}

// Lenient: models sometimes send nodes as bare strings, numbers as strings ("15,000"), edges as pairs.
fn nodes(v: &Value) -> Vec<NodeSpec> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|n| match n {
                    Value::String(l) => Some(NodeSpec { label: l.clone(), icon: None, logo: None, note: None }),
                    Value::Object(_) => Some(NodeSpec {
                        label: n["label"].as_str()?.to_string(),
                        icon: n["icon"].as_str().map(|i| i.trim().to_string()).filter(|i| !i.is_empty()),
                        logo: n["logo"].as_str().map(|i| i.trim().to_string()).filter(|i| !i.is_empty()),
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
                    Value::Array(x) if x.len() >= 2 => Some(Point { label: x[0].as_str().map(String::from).unwrap_or_else(|| x[0].to_string()), value: number(&x[1])?, icon: None, logo: None }),
                    Value::Object(_) => Some(Point {
                        label: p["label"].as_str().map(String::from).unwrap_or_else(|| p["label"].to_string()),
                        value: number(&p["value"])?,
                        icon: p["icon"].as_str().map(|i| i.trim().to_string()).filter(|i| !i.is_empty()),
                        logo: p["logo"].as_str().map(|i| i.trim().to_string()).filter(|i| !i.is_empty()),
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

    fn sentences(v: &[&str]) -> Vec<Sentence> {
        v.iter().enumerate().map(|(i, t)| Sentence { at_s: 10 * i as u64, text: t.to_string() }).collect()
    }
    fn input<'a>(scene: &'a Scene, tr: &'a [Sentence], new_from: usize, now: &'a str) -> AgentInput<'a> {
        AgentInput { scene, transcript: tr, new_from, speaking_now: now, changes: &[], now_s: 99 }
    }
    fn chart_scene() -> (Canvas, String) {
        let mut c = Canvas::new();
        let pts = |v: &[(&str, f64)]| v.iter().map(|(l, x)| Point { label: l.to_string(), value: *x, icon: None, logo: None }).collect::<Vec<_>>();
        let s = c.apply(0, &[Op::DrawChart { kind: ChartKind::Bar, title: Some("Revenue".into()), unit: Some("$M".into()), points: pts(&[("Jan", 10.0), ("Mar", 15.0)]) }], 1).unwrap();
        let id = s.elements[0].id.clone();
        (c, id)
    }

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
    fn responses_tools_are_flat_and_function_calls_parse() {
        let tools = response_tools();
        let first = &tools.as_array().unwrap()[0];
        assert_eq!(first["type"], "function");
        assert_eq!(first["name"], "no_action");
        assert!(first.get("function").is_none());
        assert_eq!(first["parameters"]["type"], "object");

        let response = json!({
            "id": "resp_1",
            "output": [
                {"type": "reasoning", "id": "rs_1"},
                {"type": "function_call", "call_id": "call_1", "name": "set_point", "arguments": "{\"id\":\"e1\",\"label\":\"March\",\"value\":80}"},
                {"type": "function_call", "call_id": "call_2", "name": "no_action", "arguments": "{\"reason\":\"done\"}"}
            ]
        });
        assert_eq!(response_call_ids(&response), vec!["call_1", "call_2"]);
        assert_eq!(
            parse_response_tool_calls(&response),
            vec![
                Op::SetPoint { id: "e1".into(), label: "March".into(), value: 80.0 },
                Op::NoAction { reason: "done".into() },
            ]
        );
    }

    #[test]
    fn parses_the_patch_photo_and_guarded_tools() {
        let call = |name: &str, args: &str| format!(r#"{{"function":{{"name":"{name}","arguments":{}}}}}"#, serde_json::to_string(args).unwrap());
        let body = format!(
            r#"{{"choices":[{{"message":{{"tool_calls":[{}]}}}}]}}"#,
            [
                call("set_point", r#"{"id":"e1","label":"Mar","value":"80"}"#),
                call("add_point", r#"{"id":"e1","label":2026,"value":95}"#),
                call("remove_point", r#"{"id":"e1","label":"Jan","quote":"drop January"}"#),
                call("set_chart", r#"{"id":"e1","kind":"line","title":"Signups"}"#),
                call("update_node", r#"{"id":"e2","node":"n2","label":"Compile"}"#),
                call("remove_node", r#"{"id":"e2","node":"Test","quote":"skip the test step"}"#),
                call("show_photo", r#"{"subject":"white rose","mode":"replace"}"#),
                call("show_photo", r#"{"subject":"owl"}"#),
                call("remove", r#"{"id":"e3","quote":"  "}"#),
                call("clear_board", r#"{"quote":"let's move on"}"#),
                call("no_action", r#"{"reason":"filler"}"#),
                call("show_logo", r#"{"name":"Google"}"#),
                call("show_icon", r#"{"concept":"teamwork","alternatives":["users","group",7]}"#),
                call("show_icon", r#"{"concept":"database"}"#),
                call("show_icon", r#"{"concept":"flag of Canada","mode":"replace"}"#),
            ]
            .join(",")
        );
        let ops = parse_tool_calls(&body);
        assert_eq!(ops.len(), 15, "{ops:?}");
        assert_eq!(ops[14], Op::ShowIcon { concept: "flag of Canada".into(), alternatives: vec![], replace: true });
        assert_eq!(ops[0], Op::SetPoint { id: "e1".into(), label: "Mar".into(), value: 80.0 });
        assert_eq!(ops[1], Op::AddPoint { id: "e1".into(), label: "2026".into(), value: 95.0, icon: None, logo: None });
        assert!(matches!(&ops[2], Op::RemovePoint { quote: Some(q), .. } if q == "drop January"));
        assert!(matches!(&ops[3], Op::SetChart { kind: Some(ChartKind::Line), title: Some(t), .. } if t == "Signups"));
        assert_eq!(ops[6], Op::ShowPhoto { subject: "white rose".into(), replace: true });
        assert_eq!(ops[7], Op::ShowPhoto { subject: "owl".into(), replace: false });
        assert!(matches!(&ops[8], Op::Remove { quote: None, .. }), "a blank quote is no quote");
        assert!(matches!(&ops[10], Op::NoAction { reason } if reason == "filler"));
        assert_eq!(ops[11], Op::ShowLogo { name: "Google".into(), replace: false });
        assert_eq!(ops[12], Op::ShowIcon { concept: "teamwork".into(), alternatives: vec!["users".into(), "group".into()], replace: false }, "non-strings are skipped");
        assert_eq!(ops[13], Op::ShowIcon { concept: "database".into(), alternatives: vec![], replace: false });
    }

    #[test]
    fn parses_rich_text_tools_and_waits_for_finished_speech() {
        let body = r#"{"choices":[{"message":{"tool_calls":[
            {"function":{"name":"draw_text","arguments":"{\"blocks\":[{\"kind\":\"heading\",\"text\":\"Three lessons\",\"emphasis\":[\"Three\"]},{\"kind\":\"bullet\",\"text\":\"Start small\"}]}"}},
            {"function":{"name":"update_text_block","arguments":"{\"id\":\"e1\",\"block\":\"b2\",\"text\":\"Start with users\"}"}},
            {"function":{"name":"remove_text_block","arguments":"{\"id\":\"e1\",\"block\":\"b2\",\"quote\":\"remove the first point\"}"}}
        ]}}]}"#;
        let ops = parse_tool_calls(body);
        assert_eq!(ops.len(), 3, "{ops:?}");
        assert!(matches!(&ops[0], Op::DrawText { blocks } if blocks.len() == 2 && blocks[0].kind == TextBlockKind::Heading));
        let scene = Scene::default();
        let (kept, dropped) = ground(vec![ops[0].clone()], &scene, "", "Three lessons", false);
        assert!(kept.is_empty() && dropped[0].contains("finished speech"));
        let (kept, _) = ground(vec![ops[0].clone()], &scene, "Three lessons.", "Three lessons.", true);
        assert_eq!(kept.len(), 1);
    }

    #[test]
    fn request_is_ordered_for_caching_and_lists_board_and_tools() {
        let (c, id) = chart_scene();
        let tr = sentences(&["Revenue was ten million in January.", "Then fifteen million in March."]);
        let a = CanvasAgent::new(reqwest::Client::new(), Some("k".into()), None);
        let v = a.request_body(&input(c.scene(), &tr, 1, "actually March was eighteen"));
        assert_eq!(v["model"], DEFAULT_MODEL);
        assert_eq!(v["tool_choice"], "required", "nothing-to-do is an explicit no_action, not silence");
        let names: Vec<&str> = v["tools"].as_array().unwrap().iter().map(|t| t["function"]["name"].as_str().unwrap()).collect();
        for n in ["no_action", "show_photo", "show_logo", "show_icon", "set_point", "add_point", "remove_point", "set_chart", "draw_text", "add_text_blocks", "update_text_block", "remove_text_block", "update_node", "remove_node", "add_nodes", "clear_board", "remove"] {
            assert!(names.contains(&n), "{n} missing from {names:?}");
        }
        assert!(!names.contains(&"update_chart") && !names.contains(&"extend_diagram"));
        let user = v["messages"][1]["content"].as_str().unwrap();
        let at = |needle: &str| user.find(needle).unwrap_or_else(|| panic!("{needle} missing from:\n{user}"));
        // stable prefix first (transcript), what changes every call last
        assert!(at("## Transcript") < at("## Board") && at("## Board") < at("## Recent changes") && at("## Recent changes") < at("## Newest words"));
        assert!(user.contains("[0:00] Revenue was ten million") && user.contains(&format!("\"id\":\"{id}\"")));
        assert!(user.contains("Said since your last call:\n[0:10] Then fifteen million in March."), "only sentences from new_from on");
        let before = &user[..at("## Board")];
        assert!(!before.contains("Then fifteen million"), "a new sentence is listed once, under the newest words: {before}");
        assert!(user.contains("Being spoken now (may be unfinished): actually March was eighteen"));

        let incremental = incremental_user_message(&input(c.scene(), &tr, 1, "actually March was eighteen"));
        assert!(!incremental.contains("Revenue was ten million"), "old transcript is inherited through previous_response_id");
        assert!(incremental.contains("Then fifteen million") && incremental.contains(&format!("\"id\":\"{id}\"")));
    }

    #[test]
    fn openai_and_openrouter_get_the_request_shape_each_one_accepts() {
        let c = Canvas::new();
        let tr = sentences(&["hello"]);
        let inp = input(c.scene(), &tr, 0, "");
        // OpenAI: bare model id, max_completion_tokens, reasoning_effort "none"; no `reasoning` / `provider` / `max_tokens`
        let oa = CanvasAgent::new(reqwest::Client::new(), None, None).with_openai(Some("k".into()));
        assert_eq!(oa.provider(), Some(Provider::OpenAi));
        let b = oa.request_body(&inp);
        assert_eq!(b["model"], "gpt-5.6-luna");
        assert_eq!(b["service_tier"], "default");
        assert_eq!((b["max_completion_tokens"].as_i64(), b["reasoning_effort"].as_str(), b["temperature"].as_i64()), (Some(700), Some("none"), Some(0)));
        for rejected in ["max_tokens", "reasoning", "provider"] {
            assert!(b.get(rejected).is_none(), "OpenAI rejects `{rejected}`");
        }
        assert_eq!(b["tool_choice"], "required");
        assert_eq!(oa.default_rpm(), 500);
        // OpenRouter: prefixed model id, max_tokens, reasoning object, provider routing
        let or = CanvasAgent::new(reqwest::Client::new(), Some("k".into()), Some("gpt-5.6-luna".into()));
        assert_eq!(or.provider(), Some(Provider::OpenRouter));
        let b = or.request_body(&inp);
        assert_eq!(b["model"], "openai/gpt-5.6-luna");
        assert_eq!((b["max_tokens"].as_i64(), b["reasoning"]["effort"].as_str()), (Some(700), Some("minimal")));
        assert!(b.get("provider").is_some() && b.get("max_completion_tokens").is_none() && b.get("reasoning_effort").is_none() && b.get("service_tier").is_none());
        assert_eq!(or.default_rpm(), 18);
        // both keys: OpenAI wins; no key: offline
        let both = CanvasAgent::new(reqwest::Client::new(), Some("r".into()), None).with_openai(Some("o".into()));
        assert_eq!(both.provider(), Some(Provider::OpenAi));
        assert_eq!(CanvasAgent::new(reqwest::Client::new(), None, None).with_openai(Some("  ".into())).provider(), None);
    }

    #[test]
    fn a_long_transcript_drops_its_oldest_sentences_not_its_newest() {
        let c = Canvas::new();
        let tr: Vec<Sentence> = (0..2000).map(|i| Sentence { at_s: i, text: format!("sentence number {i} of a very long talk about many things") }).collect();
        let msg = user_message(&input(c.scene(), &tr, 1999, ""));
        assert!(msg.len() < TRANSCRIPT_CHARS + 4_000);
        assert!(msg.contains("earlier sentences omitted") && msg.contains("sentence number 1999") && !msg.contains("sentence number 3 of"));
    }

    #[test]
    fn quotes_must_be_in_the_newest_words() {
        assert!(quote_ok("take the eagle away", "OK. Take the eagle away."));
        assert!(quote_ok("Let's move on", "so, let's move on to the next thing"));
        assert!(quote_ok("take eagle away", "Take the eagle away."), "a dropped filler word is fine");
        assert!(!quote_ok("get rid of the chart", "Take the eagle away."));
        assert!(!quote_ok("", "anything") && !quote_ok("   ", "anything"));
    }

    #[test]
    fn destructive_ops_need_a_quote_from_the_newest_words_only() {
        let (c, id) = chart_scene();
        let tr = sentences(&["Okay, let's move on.", "Revenue was ten million in January."]);
        // "let's move on" was said, but before the newest words: the model may not act on it now
        let ops = vec![Op::ClearBoard { quote: Some("let's move on".into()) }, Op::Remove { id: id.clone(), quote: None }];
        let (kept, dropped) = ground(ops, c.scene(), "Okay, let's move on. Revenue was ten million in January.", "Revenue was ten million in January.", true);
        assert!(kept.is_empty(), "{kept:?}");
        assert_eq!(dropped.len(), 2, "{dropped:?}");
        assert!(dropped[0].contains("not in the newest words") && dropped[1].contains("no quote"));
        // "take the eagle away" has no removal phrase from the old list, but the quote proves the presenter said it
        let (kept, _) = ground(vec![Op::Remove { id, quote: Some("take the chart away".into()) }], c.scene(), "", "Take the chart away.", true);
        assert_eq!(kept.len(), 1);
        let _ = tr;
    }

    #[test]
    fn chart_values_must_be_spoken_and_a_scale_word_grounds_the_bare_number() {
        let (c, id) = chart_scene(); // chart is kept in $M
        let set = |v: f64| Op::SetPoint { id: id.clone(), label: "Mar".into(), value: v };
        // "eighteen million" → 18 on a chart in millions (was dropped, and the update then lost the point)
        let (kept, _) = ground(vec![set(18.0)], c.scene(), "", "Sorry, March revenue was eighteen million.", true);
        assert_eq!(kept, vec![set(18.0)]);
        // a value nobody said is refused, with a reason
        let (kept, dropped) = ground(vec![set(99.0)], c.scene(), "", "Sorry, March revenue was eighteen million.", true);
        assert!(kept.is_empty() && dropped[0].contains("never said"), "{dropped:?}");
        // invented remainders are still dropped from a new chart
        let pie = Op::DrawChart { kind: ChartKind::Pie, title: None, unit: Some("%".into()), points: vec![Point { label: "Stoned".into(), value: 60.0, icon: None, logo: None }, Point { label: "Not stoned".into(), value: 40.0, icon: None, logo: None }] };
        let (kept, dropped) = ground(vec![pie], c.scene(), "", "About 60% of them are stoned.", true);
        match &kept[0] {
            Op::DrawChart { points, .. } => assert_eq!(points.len(), 1),
            o => panic!("{o:?}"),
        }
        assert!(dropped.iter().any(|d| d.contains("Not stoned")));
    }

    #[test]
    fn spoken_numbers_cover_digits_words_and_scales() {
        let n = spoken_numbers("Last year about two thousand users; this year 15,000, next year forty thousand. 60% are students, $1.2 million raised, two hundred and fifty stores.");
        for v in [2000.0, 15000.0, 40000.0, 60.0, 1.2, 1_200_000.0, 250.0] {
            assert!(n.iter().any(|x| (x - v).abs() < 1e-6), "{v} missing from {n:?}");
        }
        let n = spoken_numbers("March revenue was eighteen million.");
        assert!(n.contains(&18.0) && n.contains(&18_000_000.0), "both forms, as for digits: {n:?}");
    }

    #[tokio::test]
    async fn no_key_uses_rules() {
        let mut c = Canvas::new();
        c.render("eagle", "eagle", "u", 1);
        c.render("owl", "owl", "u", 2);
        let tr = sentences(&["let's compare them side by side"]);
        let a = CanvasAgent::new(reqwest::Client::new(), None, None);
        let p = a.propose(&input(c.scene(), &tr, 0, "")).await;
        assert_eq!(p.source, Source::Rules);
        assert_eq!(p.transport, Transport::Rules);
        assert_eq!(p.ops, vec![Op::Arrange { layout: Layout::Compare }]);
    }
}
