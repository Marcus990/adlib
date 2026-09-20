//! Probe harness for the canvas agent (Luna): speech in, board out.
//!
//! Each case builds a board, gives the agent some speech, applies whatever it returns to a real `Canvas`,
//! and checks the RESULTING BOARD (not which tools were called), so the same cases keep working when the
//! tool set changes. Cases live in `probes/luna/cases.json`; schema in `probes/luna/README.md`.
//!
//!   cargo run -p ls-agent --bin ls-agent-probe -- --dry            # validate cases, print boards (no API)
//!   cargo run -p ls-agent --bin ls-agent-probe -- --runs 3         # real model (OPENROUTER_API_KEY, .env ok)
//!   ... --timing --filter a,b       # no timeout: per-call latency + token usage, no pass/fail
//!   ... --filter chart-correct   --group diagram-edit   --rpm 18   --verbose   --model <id>
//!   ... --ws-latency 30         # one warmed chain of identical chart corrections; p50/p95
//!
//! Requests are paced at `--rpm` (default: the backend's, 500 on OpenAI, 18 on OpenRouter, where a new account is
//! capped at 20/min per model and a 429 would look like a model failure). OPENAI_API_KEY selects OpenAI.

use ls_agent::{AgentInput, CanvasAgent, Change, Sentence, Source, Transport};
use ls_canvas::{board_summary, Canvas, ChartKind, Op, Point, Scene};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

const KEYS: &[&str] = &[
    "elements", "present", "absent", "cleared", "no_change", "layout", "focus", "annotated", "charts", "diagrams", "texts", "photo", "no_photo", "logo", "icon", "no_symbol", "hints", "icons_valid", "point_logos", "icon_coverage",
];

#[derive(Clone)]
struct Case {
    id: String,
    group: String,
    board: Vec<Value>,
    history: Vec<String>,
    /// What was already done to the board (the "recent changes" list): `[{"at_s": 12, "what": "show_icon flag"}]`.
    changes: Vec<Change>,
    newest: String,
    expect: Value,
    pending: Option<String>,
}

struct Args {
    cases: PathBuf,
    runs: usize,
    filter: Option<String>,
    group: Option<String>,
    rpm: Option<u64>,
    dry: bool,
    verbose: bool,
    timing: bool,
    ws_smoke: bool,
    ws_latency: Option<usize>,
    model: Option<String>,
}

fn parse_args() -> Args {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut a = Args { cases: root.join("probes/luna/cases.json"), runs: 3, filter: None, group: None, rpm: None, dry: false, verbose: false, timing: false, ws_smoke: false, ws_latency: None, model: None };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--cases" => a.cases = it.next().expect("--cases <path>").into(),
            "--runs" => a.runs = it.next().and_then(|v| v.parse().ok()).expect("--runs <n>"),
            "--filter" => a.filter = it.next(),
            "--group" => a.group = it.next(),
            "--rpm" => a.rpm = Some(it.next().and_then(|v| v.parse().ok()).expect("--rpm <n>")),
            "--model" => a.model = it.next(),
            "--dry" => a.dry = true,
            "--timing" => a.timing = true,
            "--ws-smoke" => a.ws_smoke = true,
            "--ws-latency" => a.ws_latency = Some(it.next().and_then(|v| v.parse().ok()).expect("--ws-latency <turns>")),
            "--verbose" | "-v" => a.verbose = true,
            other => panic!("unknown argument {other}"),
        }
    }
    a
}

/// Minimal `.env` reader (no new dependency): fills variables that are not already set.
fn load_env() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.env");
    let Ok(text) = std::fs::read_to_string(path) else { return };
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            let v = v.trim().trim_matches('"').trim_matches('\'');
            if std::env::var(k.trim()).is_err() && !v.is_empty() {
                std::env::set_var(k.trim(), v);
            }
        }
    }
}

fn load_cases(path: &Path) -> anyhow::Result<Vec<Case>> {
    let raw: Value = serde_json::from_str(&std::fs::read_to_string(path)?)?;
    let arr = raw.as_array().ok_or_else(|| anyhow::anyhow!("{} must hold a JSON array", path.display()))?;
    let mut out = vec![];
    for c in arr {
        let s = |k: &str| c[k].as_str().map(String::from);
        let id = s("id").ok_or_else(|| anyhow::anyhow!("case without id: {c}"))?;
        out.push(Case {
            group: s("group").unwrap_or_else(|| "ungrouped".into()),
            board: c["board"].as_array().cloned().unwrap_or_default(),
            changes: c["changes"].as_array().map(|a| a.iter().map(|x| Change { at_s: x["at_s"].as_u64().unwrap_or(0), what: x["what"].as_str().unwrap_or_default().to_string() }).collect()).unwrap_or_default(),
            history: c["history"].as_array().map(|h| h.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default(),
            newest: s("newest").ok_or_else(|| anyhow::anyhow!("case {id}: no `newest`"))?,
            expect: c["expect"].clone(),
            pending: s("pending"),
            id,
        });
    }
    Ok(out)
}

/// Board setup steps, applied in order: `{"photo": "eagle"}` or `{"op": <ls_canvas::Op as JSON>}`.
fn build_canvas(case: &Case) -> anyhow::Result<Canvas> {
    let mut canvas = Canvas::new();
    for (i, step) in case.board.iter().enumerate() {
        if let Some(p) = step["photo"].as_str() {
            canvas.render(p, p, "img://probe", 0);
        } else if step.get("logo").is_some() {
            // a symbol tile already on the board: {"logo": {"asset": "lucide:flag", "caption": "Flag"}} (asset "" = a name card)
            canvas.render_logo(step["logo"]["asset"].as_str().unwrap_or_default(), step["logo"]["caption"].as_str().unwrap_or_default(), "img://probe", 0);
        } else if step.get("op").is_some() {
            let op: Op = serde_json::from_value(step["op"].clone()).map_err(|e| anyhow::anyhow!("case {} board[{i}]: {e}", case.id))?;
            let v = canvas.scene().version;
            if canvas.apply(v, &[op], 0).is_none() {
                anyhow::bail!("case {} board[{i}]: setup op changed nothing", case.id);
            }
        } else {
            anyhow::bail!("case {} board[{i}]: expected `photo` or `op`", case.id);
        }
    }
    Ok(canvas)
}

/// Everything a "no change" case must leave alone (focus and geometry are excluded on purpose).
fn fingerprint(s: &Scene) -> Value {
    json!({
        "layout": s.layout,
        "annotations": s.annotations,
        "elements": s.elements.iter().map(|e| json!({"id": e.id, "kind": e.kind, "image": e.image_id, "chart": e.chart, "diagram": e.diagram})).collect::<Vec<_>>(),
    })
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 * a.abs().max(b.abs()).max(1.0)
}

/// Serde name of an enum value ("bar", "hero", …), via `serde_json::to_value(&x)`.
fn name_of(v: serde_json::Result<Value>) -> String {
    v.ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default()
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default()
}

fn check_chart(id: &str, want: &Value, after: &Scene, bad: &mut Vec<String>) {
    let Some(chart) = after.elements.iter().find(|e| e.id == id).and_then(|e| e.chart.as_ref()) else {
        bad.push(format!("{id}: no chart on the board"));
        return;
    };
    let got = chart.points.iter().map(|p| format!("{} {}", p.label, p.value)).collect::<Vec<_>>().join(", ");
    if let Some(points) = want["points"].as_object() {
        let ok = points.len() == chart.points.len()
            && points.iter().all(|(l, v)| chart.points.iter().any(|p| p.label.trim().eq_ignore_ascii_case(l.trim()) && near(p.value, v.as_f64().unwrap_or(f64::NAN))));
        if !ok {
            bad.push(format!("{id} points: want {} got [{got}]", Value::Object(points.clone())));
        }
    }
    if let Some(values) = want["values"].as_array() {
        let mut w: Vec<f64> = values.iter().filter_map(|v| v.as_f64()).collect();
        let mut g: Vec<f64> = chart.points.iter().map(|p| p.value).collect();
        w.sort_by(|a, b| a.partial_cmp(b).unwrap());
        g.sort_by(|a, b| a.partial_cmp(b).unwrap());
        if w.len() != g.len() || !w.iter().zip(&g).all(|(a, b)| near(*a, *b)) {
            bad.push(format!("{id} values: want {w:?} got [{got}]"));
        }
    }
    if let Some(k) = want["kind"].as_str() {
        if name_of(serde_json::to_value(chart.kind)) != k {
            bad.push(format!("{id} kind: want {k} got {}", name_of(serde_json::to_value(chart.kind))));
        }
    }
    if let Some(t) = want["title_contains"].as_str() {
        if !chart.title.as_deref().unwrap_or("").to_lowercase().contains(&t.to_lowercase()) {
            bad.push(format!("{id} title: want it to contain {t:?} got {:?}", chart.title));
        }
    }
}

fn check_diagram(id: &str, want: &Value, after: &Scene, bad: &mut Vec<String>) {
    let Some(d) = after.elements.iter().find(|e| e.id == id).and_then(|e| e.diagram.as_ref()) else {
        bad.push(format!("{id}: no diagram on the board"));
        return;
    };
    let labels: Vec<String> = d.nodes.iter().map(|n| n.label.to_lowercase()).collect();
    let shown = d.nodes.iter().map(|n| n.label.as_str()).collect::<Vec<_>>().join(" → ");
    for t in strings(&want["nodes_include"]) {
        if !labels.iter().any(|l| l.contains(&t.to_lowercase())) {
            bad.push(format!("{id} nodes: want one containing {t:?} got [{shown}]"));
        }
    }
    for t in strings(&want["nodes_exclude"]) {
        if labels.iter().any(|l| l.contains(&t.to_lowercase())) {
            bad.push(format!("{id} nodes: want none containing {t:?} got [{shown}]"));
        }
    }
    if let Some(n) = want["node_count"].as_u64() {
        if d.nodes.len() != n as usize {
            bad.push(format!("{id} node count: want {n} got {} [{shown}]", d.nodes.len()));
        }
    }
    // edges: [["gateway", "auth"], …] — an edge from a node whose label contains the first to one containing the second
    let label_of = |id: &str| d.nodes.iter().find(|n| n.id == id).map(|n| n.label.to_lowercase()).unwrap_or_default();
    for pair in want["edges_include"].as_array().into_iter().flatten() {
        let (a, b) = (pair[0].as_str().unwrap_or_default().to_lowercase(), pair[1].as_str().unwrap_or_default().to_lowercase());
        if !d.edges.iter().any(|e| label_of(&e.from).contains(&a) && label_of(&e.to).contains(&b)) {
            let have = d.edges.iter().map(|e| format!("{}→{}", label_of(&e.from), label_of(&e.to))).collect::<Vec<_>>().join(", ");
            bad.push(format!("{id}: want an edge {a:?} → {b:?}, edges are [{have}]"));
        }
    }
    // either direction: the model may draw a "reads from" as data flow
    for pair in want["edges_between"].as_array().into_iter().flatten() {
        let (a, b) = (pair[0].as_str().unwrap_or_default().to_lowercase(), pair[1].as_str().unwrap_or_default().to_lowercase());
        if !d.edges.iter().any(|e| (label_of(&e.from).contains(&a) && label_of(&e.to).contains(&b)) || (label_of(&e.from).contains(&b) && label_of(&e.to).contains(&a))) {
            bad.push(format!("{id}: want an edge between {a:?} and {b:?}"));
        }
    }
    if let Some(n) = want["edge_count_min"].as_u64() {
        if (d.edges.len() as u64) < n {
            bad.push(format!("{id}: want ≥{n} edges, got {}", d.edges.len()));
        }
    }
    if let Some(l) = want["layout"].as_str() {
        if name_of(serde_json::to_value(d.layout)) != l {
            bad.push(format!("{id} diagram layout: want {l} got {}", name_of(serde_json::to_value(d.layout))));
        }
    }
}

fn check_text(id: &str, want: &Value, after: &Scene, bad: &mut Vec<String>) {
    let Some(card) = after.elements.iter().find(|e| e.id == id).and_then(|e| e.text.as_ref()) else {
        bad.push(format!("{id}: no text tile on the board"));
        return;
    };
    let shown = card.blocks.iter().map(|b| b.text.as_str()).collect::<Vec<_>>().join(" | ");
    for t in strings(&want["blocks_include"]) {
        if !card.blocks.iter().any(|b| b.text.to_lowercase().contains(&t.to_lowercase())) {
            bad.push(format!("{id} text: want a block containing {t:?}, got [{shown}]"));
        }
    }
    if let Some(n) = want["block_count"].as_u64() {
        if card.blocks.len() != n as usize { bad.push(format!("{id} block count: want {n}, got {} [{shown}]", card.blocks.len())); }
    }
}

fn check(expect: &Value, before: &Scene, after: &Scene, ops: &[Op]) -> Vec<String> {
    let mut bad = vec![];
    let Some(map) = expect.as_object() else { return vec!["`expect` must be an object".into()] };
    let has = |id: &str| after.elements.iter().any(|e| e.id == id);
    for (key, want) in map {
        match key.as_str() {
            "elements" => {
                if want.as_u64() != Some(after.elements.len() as u64) {
                    bad.push(format!("elements: want {want} got {}", after.elements.len()));
                }
            }
            "present" => strings(want).iter().filter(|id| !has(id)).for_each(|id| bad.push(format!("{id} should still be on the board"))),
            "absent" => strings(want).iter().filter(|id| has(id)).for_each(|id| bad.push(format!("{id} should have been removed"))),
            "cleared" => {
                if want.as_bool() == Some(true) && !after.elements.is_empty() {
                    bad.push(format!("board should be clear, {} tile(s) remain", after.elements.len()));
                }
            }
            "no_change" => {
                if want.as_bool() == Some(true) && fingerprint(before) != fingerprint(after) {
                    bad.push("board should be unchanged but was modified".into());
                }
            }
            "layout" => {
                if name_of(serde_json::to_value(after.layout)) != want.as_str().unwrap_or_default() {
                    bad.push(format!("layout: want {want} got {}", name_of(serde_json::to_value(after.layout))));
                }
            }
            "focus" => {
                let f = after.elements.iter().find(|e| e.focus).map(|e| e.id.clone());
                if f.as_deref() != want.as_str() {
                    bad.push(format!("focus: want {want} got {f:?}"));
                }
            }
            "annotated" => {
                for id in strings(want) {
                    if !after.annotations.iter().any(|a| a.targets.contains(&id)) {
                        bad.push(format!("{id} should be annotated"));
                    }
                }
            }
            "charts" => want.as_object().into_iter().flatten().for_each(|(id, w)| check_chart(id, w, after, &mut bad)),
            "diagrams" => want.as_object().into_iter().flatten().for_each(|(id, w)| check_diagram(id, w, after, &mut bad)),
            "texts" => want.as_object().into_iter().flatten().for_each(|(id, w)| check_text(id, w, after, &mut bad)),
            "photo" => {
                let subject = want["subject_contains"].as_str().unwrap_or_default().to_lowercase();
                let replace = want["mode"].as_str() == Some("replace");
                let asked: Vec<String> = ops.iter().filter_map(|o| if let Op::ShowPhoto { subject, replace } = o { Some(format!("{subject}{}", if *replace { " (replace)" } else { "" })) } else { None }).collect();
                let ok = ops.iter().any(|o| matches!(o, Op::ShowPhoto { subject: s, replace: r } if s.to_lowercase().contains(&subject) && *r == replace));
                if !ok {
                    bad.push(format!("photo: want {subject:?} ({}) got {asked:?}", if replace { "replace" } else { "add" }));
                }
            }
            "hints" | "icons_valid" | "icon_coverage" | "point_logos" => {
                // what the model asked for on nodes and points (the pipeline resolves these; the probe checks the asking)
                let nodes: Vec<&ls_canvas::NodeSpec> = ops.iter().flat_map(|o| match o { Op::DrawDiagram { nodes, .. } | Op::AddNodes { nodes, .. } => nodes.iter().collect::<Vec<_>>(), _ => vec![] }).collect();
                let points: Vec<&ls_canvas::Point> = ops.iter().flat_map(|o| match o { Op::DrawChart { points, .. } => points.iter().collect::<Vec<_>>(), _ => vec![] }).collect();
                let vocab: Vec<&str> = ls_agent::ICON_NAMES.split_whitespace().collect();
                match key.as_str() {
                    "hints" => {
                        for (label, spec) in want.as_object().into_iter().flatten() {
                            let Some(n) = nodes.iter().find(|n| n.label.to_lowercase().contains(&label.to_lowercase())) else { bad.push(format!("hints: no node like {label:?}")); continue };
                            if let Some(l) = spec["logo"].as_str() {
                                if !n.logo.as_deref().unwrap_or_default().to_lowercase().contains(&l.to_lowercase()) {
                                    bad.push(format!("hints: {label:?} should carry logo {l:?}, has logo={:?} icon={:?}", n.logo, n.icon));
                                }
                            }
                            if let Some(any) = spec["icon_any"].as_array() {
                                let ok = n.icon.as_deref().is_some_and(|i| any.iter().any(|a| a.as_str() == Some(i)));
                                if !ok {
                                    bad.push(format!("hints: {label:?} should carry an icon among {any:?}, has icon={:?} logo={:?}", n.icon, n.logo));
                                }
                            }
                        }
                    }
                    "icons_valid" => {
                        for n in &nodes {
                            if let Some(i) = n.icon.as_deref() {
                                if !vocab.contains(&i) {
                                    bad.push(format!("icon {i:?} on {:?} is not in the icon list", n.label));
                                }
                            }
                        }
                    }
                    "icon_coverage" => {
                        let min = want.as_f64().unwrap_or(0.0);
                        let with = nodes.iter().filter(|n| n.icon.is_some() || n.logo.is_some()).count();
                        if nodes.is_empty() || (with as f64) < min * nodes.len() as f64 {
                            bad.push(format!("icon coverage: {with}/{} nodes have a logo or icon, want ≥{:.0}%", nodes.len(), min * 100.0));
                        }
                    }
                    _ => {
                        // brand: one name, or a list of acceptable names ("aws" or "amazon")
                        for (label, brand) in want.as_object().into_iter().flatten() {
                            let names: Vec<String> = match brand { serde_json::Value::Array(a) => a.iter().filter_map(|x| x.as_str().map(|x| x.to_lowercase())).collect(), b => vec![b.as_str().unwrap_or_default().to_lowercase()] };
                            let ok = points.iter().find(|p| p.label.to_lowercase().contains(&label.to_lowercase())).is_some_and(|p| { let l = p.logo.as_deref().unwrap_or_default().to_lowercase(); names.iter().any(|n| l.contains(n)) });
                            if !ok {
                                bad.push(format!("point_logos: {label:?} should carry logo {brand:?}"));
                            }
                        }
                    }
                }
            }
            "logo" => {
                // one expectation, or a list of them (a sentence that names several brands)
                let wants: Vec<&Value> = if want.is_array() { want.as_array().unwrap().iter().collect() } else { vec![want] };
                let asked: Vec<(&str, bool)> = ops.iter().filter_map(|o| if let Op::ShowLogo { name, replace } = o { Some((name.as_str(), *replace)) } else { None }).collect();
                for want in wants {
                    let name = want["name_contains"].as_str().unwrap_or_default().to_lowercase();
                    let replace = want["replace"].as_bool();
                    if !asked.iter().any(|(n, r)| n.to_lowercase().contains(&name) && replace.is_none_or(|w| w == *r)) {
                        bad.push(format!("logo: want show_logo containing {name:?}{} got {asked:?}", replace.map_or(String::new(), |w| format!(" with replace={w}"))));
                    }
                }
            }
            "icon" => {
                let concept = want["concept_contains"].as_str().unwrap_or_default().to_lowercase();
                let asked: Vec<(&str, usize, bool)> = ops.iter().filter_map(|o| if let Op::ShowIcon { concept, alternatives, replace } = o { Some((concept.as_str(), alternatives.len(), *replace)) } else { None }).collect();
                let min_alt = want["min_alternatives"].as_u64().unwrap_or(0) as usize;
                let replace = want["replace"].as_bool();
                if !asked.iter().any(|(c, a, r)| c.to_lowercase().contains(&concept) && *a >= min_alt && replace.is_none_or(|w| w == *r)) {
                    bad.push(format!("icon: want show_icon containing {concept:?} with ≥{min_alt} synonyms{}, got {asked:?}", replace.map_or(String::new(), |w| format!(" and replace={w}"))));
                }
            }
            "no_symbol" => {
                if want.as_bool() == Some(true) && ops.iter().any(|o| matches!(o, Op::ShowLogo { .. } | Op::ShowIcon { .. })) {
                    bad.push("no logo or icon should have been requested".into());
                }
            }
            "no_photo" => {
                if want.as_bool() == Some(true) && ops.iter().any(|o| matches!(o, Op::ShowPhoto { .. })) {
                    bad.push("no photo should have been requested".into());
                }
            }
            other => bad.push(format!("unknown expectation key {other:?}")),
        }
    }
    bad
}

struct Outcome {
    source: Source,
    transport: Transport,
    service_tier: Option<String>,
    first_event_ms: Option<u64>,
    ms: u128,
    ops: Vec<Op>,
    dropped: Vec<String>,
    notes: Vec<String>,
    error: Option<String>,
    problems: Vec<String>,
}

/// The case as one agent call: the history and the newest sentence are finished sentences (5 s apart) and only
/// the newest is "new since your last call", which is what the live pipeline sends when a final sentence lands.
fn input_for<'a>(scene: &'a Scene, transcript: &'a [Sentence], changes: &'a [Change]) -> AgentInput<'a> {
    AgentInput { scene, transcript, new_from: transcript.len() - 1, speaking_now: "", changes, now_s: 5 * transcript.len() as u64 }
}

fn transcript_of(case: &Case) -> Vec<Sentence> {
    case.history.iter().chain(std::iter::once(&case.newest)).enumerate().map(|(i, t)| Sentence { at_s: 5 * i as u64, text: t.clone() }).collect()
}

async fn run_once(agent: &CanvasAgent, case: &Case) -> anyhow::Result<Outcome> {
    let mut canvas = build_canvas(case)?;
    let before = canvas.scene().clone();
    let transcript = transcript_of(case);
    let t = Instant::now();
    let p = agent.propose(&input_for(&before, &transcript, &case.changes)).await;
    let ms = t.elapsed().as_millis();
    let after = canvas.apply_seen(&before, &p.ops, 1).unwrap_or_else(|| before.clone());
    let notes = canvas.take_notes();
    let problems = check(&case.expect, &before, &after, &p.ops);
    Ok(Outcome {
        source: p.source,
        transport: p.transport,
        service_tier: p.service_tier,
        first_event_ms: p.first_event_ms,
        ms,
        ops: p.ops,
        dropped: p.dropped,
        notes,
        error: p.error,
        problems,
    })
}

/// Two related generated turns on one socket: proves warmup chaining, function_call_output, incremental speech,
/// and a patch op against the board produced by the first turn.
async fn run_ws_smoke(agent: &CanvasAgent) -> anyhow::Result<()> {
    anyhow::ensure!(agent.websocket_enabled(), "--ws-smoke requires OPENAI_API_KEY and cannot run with CANVAS_TRANSPORT=http");
    agent.reset_websocket().await;
    let warm_ms = agent.warm_up().await?.unwrap_or_default();
    let mut canvas = Canvas::new();
    let mut transcript = vec![Sentence {
        at_s: 0,
        text: "January signups were forty, February fifty-five, and March seventy.".into(),
    }];
    let mut changes: Vec<Change> = vec![];

    for (turn, new_from) in [(1u64, 0usize), (2u64, 1usize)] {
        if turn == 2 {
            transcript.push(Sentence { at_s: 5, text: "Actually, let's correct March from seventy to eighty.".into() });
        }
        let before = canvas.scene().clone();
        let input = AgentInput { scene: &before, transcript: &transcript, new_from, speaking_now: "", changes: &changes, now_s: 5 * (turn - 1) };
        let started = Instant::now();
        let proposal = agent.propose(&input).await;
        anyhow::ensure!(proposal.transport == Transport::ResponsesWebSocket, "turn {turn} used {:?}: {:?}", proposal.transport, proposal.error);
        let _ = canvas.apply_seen(&before, &proposal.ops, turn);
        let applied = canvas.take_applied();
        let refused = canvas.take_notes();
        agent.acknowledge(&applied, &refused, canvas.scene()).await;
        changes.extend(applied.iter().map(|what| Change { at_s: 5 * (turn - 1), what: what.clone() }));
        println!(
            "turn {turn}: {} ms · tier {:?} · first event {:?} ms · ops {} · applied {:?}",
            started.elapsed().as_millis(),
            proposal.service_tier,
            proposal.first_event_ms,
            serde_json::to_string(&proposal.ops)?,
            applied
        );
    }
    let chart = canvas.scene().elements.iter().find_map(|element| element.chart.as_ref()).ok_or_else(|| anyhow::anyhow!("smoke test produced no chart"))?;
    let march = chart.points.iter().find(|point| point.label.eq_ignore_ascii_case("March") || point.label.eq_ignore_ascii_case("Mar"));
    anyhow::ensure!(march.is_some_and(|point| near(point.value, 80.0)), "March was not corrected to 80: {:?}", chart.points);
    println!("PASS ws-smoke · warmup {warm_ms} ms · final board {}", board_summary(canvas.scene()).join(" | "));
    Ok(())
}

fn percentile(samples: &mut [u128], pct: usize) -> u128 {
    samples.sort_unstable();
    let rank = (samples.len() * pct).div_ceil(100).saturating_sub(1);
    samples[rank]
}

/// Measure one persistent Responses chain with a stable task shape. Alternating the same chart correction keeps
/// output size comparable while still exercising function_call_output, board state, and previous_response_id.
async fn run_ws_latency(agent: &CanvasAgent, turns: usize) -> anyhow::Result<()> {
    anyhow::ensure!(turns >= 2, "--ws-latency needs at least 2 turns");
    anyhow::ensure!(agent.websocket_enabled(), "--ws-latency requires OPENAI_API_KEY and CANVAS_TRANSPORT=websocket");
    agent.reset_websocket().await;
    let warm_ms = agent.warm_up().await?.unwrap_or_default();

    let mut canvas = Canvas::new();
    let initial = Op::DrawChart {
        kind: ChartKind::Bar,
        title: Some("Monthly signups".into()),
        unit: Some("signups".into()),
        points: vec![Point { label: "March".into(), value: 70.0, icon: None, logo: None }],
    };
    let _ = canvas.apply(0, &[initial], 0);
    let mut transcript: Vec<Sentence> = vec![];
    let mut changes: Vec<Change> = vec![];
    let mut totals: Vec<u128> = vec![];
    let mut first_events: Vec<u128> = vec![];
    let mut after_first: Vec<u128> = vec![];
    let mut served_tier: Option<String> = None;

    for turn in 1..=turns {
        let (old, target) = if turn % 2 == 1 { (70, 80) } else { (80, 70) };
        transcript.push(Sentence {
            at_s: turn as u64 * 5,
            text: format!("Actually, correct March from {old} to {target}."),
        });
        let before = canvas.scene().clone();
        let input = AgentInput {
            scene: &before,
            transcript: &transcript,
            new_from: transcript.len() - 1,
            speaking_now: "",
            changes: &changes,
            now_s: turn as u64 * 5,
        };
        let started = Instant::now();
        let proposal = agent.propose(&input).await;
        let total = started.elapsed().as_millis();
        anyhow::ensure!(proposal.transport == Transport::ResponsesWebSocket, "turn {turn} used {:?}: {:?}", proposal.transport, proposal.error);
        let tier = proposal.service_tier.as_deref().ok_or_else(|| anyhow::anyhow!("turn {turn} did not report a service tier"))?;
        if let Some(expected) = served_tier.as_deref() {
            anyhow::ensure!(tier == expected, "turn {turn} changed service tier from {expected} to {tier}");
        } else {
            served_tier = Some(tier.to_string());
        }
        let first = proposal.first_event_ms.ok_or_else(|| anyhow::anyhow!("turn {turn} had no first-event timing"))? as u128;

        let _ = canvas.apply_seen(&before, &proposal.ops, turn as u64);
        let applied = canvas.take_applied();
        let refused = canvas.take_notes();
        let value = canvas
            .scene()
            .elements
            .iter()
            .find_map(|element| element.chart.as_ref())
            .and_then(|chart| chart.points.iter().find(|point| point.label.eq_ignore_ascii_case("March")))
            .map(|point| point.value);
        anyhow::ensure!(value.is_some_and(|value| near(value, target as f64)), "turn {turn} did not set March to {target}: ops {:?}, refused {:?}", proposal.ops, refused);
        agent.acknowledge(&applied, &refused, canvas.scene()).await;
        changes.extend(applied.into_iter().map(|what| Change { at_s: turn as u64 * 5, what }));
        if changes.len() > 10 {
            changes.drain(..changes.len() - 10);
        }

        totals.push(total);
        first_events.push(first);
        after_first.push(total.saturating_sub(first));
        println!("turn {turn:>2}: {total:>4} ms total · {first:>4} ms first event · {:>4} ms after · March {target}", total.saturating_sub(first));
    }

    let mut total_p50 = totals.clone();
    let mut total_p95 = totals.clone();
    let mut first_p50 = first_events.clone();
    let mut first_p95 = first_events.clone();
    let mut after_p50 = after_first.clone();
    let mut after_p95 = after_first.clone();
    println!("\n{} WebSocket chain · {turns} turns · warmup {warm_ms} ms", served_tier.as_deref().unwrap_or("unknown tier"));
    println!("total       p50 {:>4} ms · p95 {:>4} ms", percentile(&mut total_p50, 50), percentile(&mut total_p95, 95));
    println!("first event p50 {:>4} ms · p95 {:>4} ms", percentile(&mut first_p50, 50), percentile(&mut first_p95, 95));
    println!("after event p50 {:>4} ms · p95 {:>4} ms", percentile(&mut after_p50, 50), percentile(&mut after_p95, 95));
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    load_env();
    let args = parse_args();
    let cases: Vec<Case> = load_cases(&args.cases)?
        .into_iter()
        .filter(|c| args.filter.as_ref().is_none_or(|f| f.split(',').any(|p| c.id.contains(p))))
        .filter(|c| args.group.as_ref().is_none_or(|g| &c.group == g))
        .collect();

    // Validate every case (setup applies, expectation keys known) before spending any API calls.
    let mut invalid = 0;
    for c in &cases {
        let problem = match build_canvas(c) {
            Err(e) => Some(e.to_string()),
            Ok(canvas) => {
                let unknown: Vec<&str> = c.expect.as_object().map(|m| m.keys().map(|k| k.as_str()).filter(|k| !KEYS.contains(k)).collect()).unwrap_or_default();
                if args.dry {
                    println!("{:<28} [{}]{}", c.id, c.group, c.pending.as_ref().map(|p| format!("  PENDING: {p}")).unwrap_or_default());
                    for line in board_summary(canvas.scene()) {
                        println!("    board: {line}");
                    }
                    println!("    said:  {}", c.newest);
                }
                (!unknown.is_empty()).then(|| format!("unknown expectation keys {unknown:?}"))
            }
        };
        if let Some(p) = problem {
            invalid += 1;
            eprintln!("INVALID {}: {p}", c.id);
        }
    }
    if invalid > 0 {
        anyhow::bail!("{invalid} invalid case(s)");
    }
    let (runnable, pending): (Vec<&Case>, Vec<&Case>) = cases.iter().partition(|c| c.pending.is_none());
    if args.dry {
        println!("\n{} cases ok ({} runnable, {} pending)", cases.len(), runnable.len(), pending.len());
        return Ok(());
    }

    let key = std::env::var("OPENROUTER_API_KEY").ok().filter(|k| !k.trim().is_empty());
    let openai = std::env::var("OPENAI_API_KEY").ok().filter(|k| !k.trim().is_empty());
    anyhow::ensure!(key.is_some() || openai.is_some(), "neither OPENAI_API_KEY nor OPENROUTER_API_KEY is set (env or .env); use --dry to only validate the cases");
    let model = args.model.clone().or_else(|| std::env::var("CANVAS_MODEL").ok());
    let agent = CanvasAgent::new(reqwest::Client::new(), key, model).with_openai(openai);
    let rpm = args.rpm.unwrap_or(agent.default_rpm() as u64);

    if args.ws_smoke {
        println!("{:?} · model {} · stateful WebSocket smoke\n", agent.provider().unwrap(), agent.wire_model());
        return run_ws_smoke(&agent).await;
    }
    if let Some(turns) = args.ws_latency {
        println!("{:?} · model {} · stateful WebSocket latency ({turns} turns)\n", agent.provider().unwrap(), agent.wire_model());
        return run_ws_latency(&agent, turns).await;
    }
    println!("{:?} · model {} · {} cases × {} runs · {} pending skipped\n", agent.provider().unwrap(), agent.wire_model(), runnable.len(), args.runs, pending.len());

    if args.timing {
        println!("{:<34} {:>7} {:>8} {:>8} {:>9} {:>6}  provider", "case", "ms", "prompt", "output", "reasoning", "calls");
        let interval = std::time::Duration::from_millis(60_000 / rpm.max(1));
        for case in &runnable {
            for _ in 0..args.runs {
                let canvas = build_canvas(case)?;
                let transcript = transcript_of(case);
                let t = Instant::now();
                let raw = agent.call_raw(&input_for(canvas.scene(), &transcript, &case.changes)).await;
                let ms = t.elapsed().as_millis();
                match raw.and_then(|r| Ok(serde_json::from_str::<Value>(&r)?)) {
                    Ok(v) => println!(
                        "{:<34} {:>7} {:>8} {:>8} {:>9} {:>6}  {}",
                        case.id, ms, v["usage"]["prompt_tokens"], v["usage"]["completion_tokens"],
                        v["usage"]["completion_tokens_details"]["reasoning_tokens"],
                        v["choices"][0]["message"]["tool_calls"].as_array().map_or(0, |a| a.len()),
                        v["provider"].as_str().unwrap_or("?")
                    ),
                    Err(e) => println!("{:<34} {:>7}  ERROR {e:#}", case.id, ms),
                }
                tokio::time::sleep(interval.saturating_sub(t.elapsed())).await;
            }
        }
        return Ok(());
    }

    let jobs: Vec<(usize, usize)> = (0..runnable.len()).flat_map(|c| (0..args.runs).map(move |r| (c, r))).collect();
    let interval = std::time::Duration::from_millis(60_000 / rpm.max(1));
    println!("pacing {} requests at {} per minute (~{:.1} min)\n", jobs.len(), rpm, jobs.len() as f64 / rpm.max(1) as f64);
    let mut results: Vec<(usize, usize, Outcome)> = vec![];
    if agent.websocket_enabled() {
        // Probe cases are independent conversations. A live talk shares one chain, but a probe must never see
        // another case's board or transcript, so reset and run them serially on the persistent transport.
        for (n, &(ci, ri)) in jobs.iter().enumerate() {
            if n > 0 {
                tokio::time::sleep(interval).await;
            }
            agent.reset_websocket().await;
            agent.warm_up().await?;
            results.push((ci, ri, run_once(&agent, runnable[ci]).await?));
        }
    } else {
        let handles: Vec<_> = jobs
            .iter()
            .enumerate()
            .map(|(n, &(ci, ri))| {
                let (agent, case) = (agent.clone(), runnable[ci].clone());
                tokio::spawn(async move {
                    tokio::time::sleep(interval * n as u32).await;
                    run_once(&agent, &case).await.map(|o| (ci, ri, o))
                })
            })
            .collect();
        for h in handles {
            results.push(h.await??);
        }
    }

    // pass = model answered and every expectation held; error = the model call failed and the agent fell
    // back to offline rules (that says nothing about the model, so it is neither pass nor fail).
    let mut by_group: BTreeMap<String, [usize; 3]> = BTreeMap::new();
    let mut failing: Vec<String> = vec![];
    let mut latencies: Vec<u128> = vec![];
    for (ci, case) in runnable.iter().enumerate() {
        let runs: Vec<&(usize, usize, Outcome)> = results.iter().filter(|r| r.0 == ci).collect();
        let g = by_group.entry(case.group.clone()).or_default();
        let (mut pass, mut fail, mut err) = (0, 0, 0);
        for (_, _, o) in &runs {
            if o.source == Source::Rules {
                err += 1;
            } else if o.problems.is_empty() {
                pass += 1;
                latencies.push(o.ms);
            } else {
                fail += 1;
                latencies.push(o.ms);
            }
        }
        g[0] += pass;
        g[1] += fail;
        g[2] += err;
        let mark = if fail + err == 0 { "PASS" } else if pass == 0 { "FAIL" } else { "FLAKY" };
        println!("{mark:<5} {:<28} {pass}/{}{}", case.id, runs.len(), if err > 0 { format!("  ({err} model errors)") } else { String::new() });
        if fail > 0 || err > 0 || args.verbose {
            if let Some((_, _, o)) = runs.iter().find(|r| !r.2.problems.is_empty() || r.2.source == Source::Rules).or(runs.first()) {
                println!("        said: {}", case.newest);
                println!("        transport: {:?} · tier {}{}", o.transport, o.service_tier.as_deref().unwrap_or("?"), o.first_event_ms.map(|ms| format!(" (first event {ms} ms)")).unwrap_or_default());
                println!("        ops:  {}", serde_json::to_string(&o.ops).unwrap_or_default());
                for d in o.dropped.iter().chain(o.notes.iter()) {
                    println!("        refused: {d}");
                }
                if o.source == Source::Rules {
                    println!("        model call failed ({}) — fell back to offline rules", o.error.as_deref().unwrap_or("?"));
                }
                for p in &o.problems {
                    println!("        ✗ {p}");
                }
            }
        }
        if fail > 0 {
            failing.push(case.id.clone());
        }
    }

    println!("\n{:<16} {:>5} {:>5} {:>7}", "group", "pass", "fail", "errors");
    let mut tot = [0usize; 3];
    for (g, v) in &by_group {
        println!("{g:<16} {:>5} {:>5} {:>7}", v[0], v[1], v[2]);
        (0..3).for_each(|i| tot[i] += v[i]);
    }
    println!("{:<16} {:>5} {:>5} {:>7}", "TOTAL", tot[0], tot[1], tot[2]);
    latencies.sort();
    if !latencies.is_empty() {
        println!("agent latency p50 {} ms · max {} ms", latencies[latencies.len() / 2], latencies[latencies.len() - 1]);
    }
    if !failing.is_empty() {
        std::process::exit(1);
    }
    Ok(())
}
