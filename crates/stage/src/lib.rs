//! Track E (logic): join the two branches by chunk id and run the stage state machine
//! (design doc §7.4 join rules, §7.5 stage rules). Pure and deterministic: the caller passes
//! `now_ms` into every call, so the whole thing is unit-testable without clocks or threads.

use ls_contracts::{Action, ChangeDecision, Chunk, Displayed, Match, Rect, RenderEvent};
use std::collections::{BTreeMap, VecDeque};

/// Result of branch 2 (query model + search) for one chunk. `best == None` means the search ran
/// but nothing cleared τ (a library gap). Internal to the join; not a shared contract.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchOutcome {
    pub chunk_id: u64,
    pub best: Option<Match>,
}

#[derive(Debug, Clone, Copy)]
pub struct StageConfig {
    pub p_render: f32,
    pub p_update: f32,
    pub p_clear: f32,
    /// τ: minimum match score to show an image.
    pub tau: f32,
    pub hold_render_ms: u64,
    pub hold_update_ms: u64,
    /// Drop a chunk if one branch arrives and the other is still missing this long after.
    pub join_timeout_ms: u64,
}

impl Default for StageConfig {
    fn default() -> Self {
        Self {
            p_render: 0.6,
            p_update: 0.6,
            p_clear: 0.7,
            tau: 0.475,
            hold_render_ms: 4000,
            hold_update_ms: 1500,
            join_timeout_ms: 1000,
        }
    }
}

/// Why a joined chunk did or did not change the screen (for the JSONL log / debug window).
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Rendered(RenderEvent),
    Pending,
    NoChange,
    BelowProbability,
    LibraryGap,
    Duplicate,
    Stale,
    TimedOut,
}

#[derive(Debug, Clone, PartialEq)]
struct Visual {
    kind: &'static str,
    image_id: Option<String>,
    caption: Option<String>,
    trigger_text: String,
    chunk_id: u64,
}

#[derive(Default)]
struct Half {
    decision: Option<ChangeDecision>,
    search: Option<SearchOutcome>,
    first_seen_ms: u64,
}

pub struct Stage {
    cfg: StageConfig,
    halves: BTreeMap<u64, Half>,
    texts: VecDeque<(u64, String)>,
    current: Visual,
    current_since_ms: u64,
    pending: Option<Visual>,
    last_applied_seq: Option<u64>,
}

impl Stage {
    pub fn new(cfg: StageConfig) -> Self {
        Self {
            cfg,
            halves: BTreeMap::new(),
            texts: VecDeque::new(),
            current: Visual { kind: "clear", image_id: None, caption: None, trigger_text: String::new(), chunk_id: 0 },
            current_since_ms: 0,
            pending: None,
            last_applied_seq: None,
        }
    }

    pub fn config(&self) -> &StageConfig {
        &self.cfg
    }

    /// Remember chunk text so the visual it triggers can carry `trigger_text`.
    pub fn on_chunk(&mut self, c: &Chunk) {
        self.texts.retain(|(id, _)| *id != c.id);
        self.texts.push_back((c.id, c.text.clone()));
        while self.texts.len() > 64 {
            self.texts.pop_front();
        }
    }

    /// What Jev and the query model should see as "on screen": the pending visual if a change is
    /// waiting out the hold, otherwise the current one (§7.1).
    pub fn displayed(&self) -> Displayed {
        let v = self.pending.as_ref().unwrap_or(&self.current);
        Displayed {
            image_id: v.image_id.clone(),
            caption: v.caption.clone(),
            trigger_text: v.trigger_text.clone(),
            shown_at_ms: self.current_since_ms,
        }
    }

    pub fn on_decision(&mut self, d: ChangeDecision, now_ms: u64) -> Option<Outcome> {
        let id = d.chunk_id;
        let h = self.halves.entry(id).or_insert_with(|| Half { first_seen_ms: now_ms, ..Default::default() });
        h.decision = Some(d);
        self.try_join(id, now_ms)
    }

    pub fn on_search(&mut self, s: SearchOutcome, now_ms: u64) -> Option<Outcome> {
        let id = s.chunk_id;
        let h = self.halves.entry(id).or_insert_with(|| Half { first_seen_ms: now_ms, ..Default::default() });
        h.search = Some(s);
        self.try_join(id, now_ms)
    }

    /// Call periodically (e.g. every 50 ms). Expires half-joined chunks and promotes the pending
    /// visual once the hold has elapsed. Returns (chunk_id, outcome) pairs for logging.
    pub fn tick(&mut self, now_ms: u64) -> Vec<(u64, Outcome)> {
        let mut out = vec![];
        let expired: Vec<u64> = self
            .halves
            .iter()
            .filter(|(_, h)| now_ms.saturating_sub(h.first_seen_ms) > self.cfg.join_timeout_ms)
            .map(|(id, _)| *id)
            .collect();
        for id in expired {
            self.halves.remove(&id);
            out.push((id, Outcome::TimedOut));
        }
        if let Some(p) = self.pending.clone() {
            if now_ms >= self.current_since_ms + self.hold_for(p.kind) {
                self.pending = None;
                let ev = self.show(p, now_ms);
                out.push((ev.chunk_id, Outcome::Rendered(ev)));
            }
        }
        out
    }

    fn hold_for(&self, next_kind: &str) -> u64 {
        if next_kind == "update" { self.cfg.hold_update_ms } else { self.cfg.hold_render_ms }
    }

    fn try_join(&mut self, id: u64, now_ms: u64) -> Option<Outcome> {
        let ready = self.halves.get(&id).map(|h| h.decision.is_some() && h.search.is_some())?;
        if !ready {
            return None;
        }
        let h = self.halves.remove(&id)?;
        let (d, s) = (h.decision?, h.search?);
        Some(self.apply(d, s, now_ms))
    }

    fn apply(&mut self, d: ChangeDecision, s: SearchOutcome, now_ms: u64) -> Outcome {
        if let Some(last) = self.last_applied_seq {
            if d.seq <= last {
                return Outcome::Stale;
            }
        }
        let trigger_text = self.texts.iter().find(|(i, _)| *i == d.chunk_id).map(|(_, t)| t.clone()).unwrap_or_default();
        let next = match d.action {
            Action::NoChange => return Outcome::NoChange,
            Action::Clear => {
                if d.p < self.cfg.p_clear {
                    return Outcome::BelowProbability;
                }
                let on_screen = self.pending.as_ref().unwrap_or(&self.current);
                if on_screen.image_id.is_none() {
                    return Outcome::Duplicate;
                }
                Visual { kind: "clear", image_id: None, caption: None, trigger_text, chunk_id: d.chunk_id }
            }
            Action::NewRender | Action::Update => {
                let (min_p, kind) = if d.action == Action::NewRender {
                    (self.cfg.p_render, "render")
                } else {
                    (self.cfg.p_update, "update")
                };
                if d.p < min_p {
                    return Outcome::BelowProbability;
                }
                let m = match s.best {
                    Some(m) if m.score >= self.cfg.tau => m,
                    _ => return Outcome::LibraryGap,
                };
                let dup = |v: &Visual| v.image_id.as_deref() == Some(m.image_id.as_str());
                if dup(&self.current) || self.pending.as_ref().map(dup).unwrap_or(false) {
                    return Outcome::Duplicate;
                }
                Visual { kind, image_id: Some(m.image_id), caption: Some(m.caption), trigger_text, chunk_id: d.chunk_id }
            }
        };
        self.last_applied_seq = Some(d.seq);
        let hold = self.hold_for(next.kind);
        let first_visual = self.current.image_id.is_none() && self.current.chunk_id == 0;
        if first_visual || now_ms >= self.current_since_ms + hold {
            self.pending = None;
            Outcome::Rendered(self.show(next, now_ms))
        } else {
            // One pending slot; a newer change replaces an older one.
            self.pending = Some(next);
            Outcome::Pending
        }
    }

    fn show(&mut self, v: Visual, now_ms: u64) -> RenderEvent {
        let ev = RenderEvent {
            kind: v.kind,
            url: v.image_id.as_ref().map(|id| format!("img://localhost/{id}")),
            image_id: v.image_id.clone(),
            rect: Rect::FULL,
            chunk_id: v.chunk_id,
            ts_ms: now_ms,
        };
        self.current = v;
        self.current_since_ms = now_ms;
        ev
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dec(chunk: u64, seq: u64, action: Action, p: f32) -> ChangeDecision {
        ChangeDecision { chunk_id: chunk, seq, action, p }
    }
    fn found(chunk: u64, id: &str, score: f32) -> SearchOutcome {
        SearchOutcome {
            chunk_id: chunk,
            best: Some(Match { chunk_id: chunk, image_id: id.into(), caption: id.into(), score, phrase: id.into() }),
        }
    }
    fn rendered(o: Option<Outcome>) -> RenderEvent {
        match o {
            Some(Outcome::Rendered(ev)) => ev,
            other => panic!("expected render, got {other:?}"),
        }
    }

    #[test]
    fn first_render_is_immediate_either_order() {
        let mut s = Stage::new(StageConfig::default());
        assert_eq!(s.on_search(found(1, "eagle", 0.6), 100), None);
        let ev = rendered(s.on_decision(dec(1, 1, Action::NewRender, 0.9), 150));
        assert_eq!(ev.kind, "render");
        assert_eq!(ev.image_id.as_deref(), Some("eagle"));
        assert_eq!(ev.url.as_deref(), Some("img://localhost/eagle"));
        assert_eq!(ev.rect, Rect::FULL);
    }

    #[test]
    fn no_change_and_low_probability_do_nothing() {
        let mut s = Stage::new(StageConfig::default());
        s.on_search(found(1, "eagle", 0.6), 0);
        assert_eq!(s.on_decision(dec(1, 1, Action::NoChange, 0.99), 0), Some(Outcome::NoChange));
        s.on_search(found(2, "eagle", 0.6), 0);
        assert_eq!(s.on_decision(dec(2, 2, Action::NewRender, 0.5), 0), Some(Outcome::BelowProbability));
    }

    #[test]
    fn below_tau_is_library_gap() {
        let mut s = Stage::new(StageConfig::default());
        s.on_decision(dec(1, 1, Action::NewRender, 0.9), 0);
        assert_eq!(s.on_search(found(1, "eagle", 0.4), 0), Some(Outcome::LibraryGap));
        s.on_decision(dec(2, 2, Action::NewRender, 0.9), 0);
        assert_eq!(s.on_search(SearchOutcome { chunk_id: 2, best: None }, 0), Some(Outcome::LibraryGap));
    }

    #[test]
    fn hold_puts_change_in_pending_then_tick_promotes() {
        let mut s = Stage::new(StageConfig::default());
        s.on_search(found(1, "eagle", 0.6), 0);
        rendered(s.on_decision(dec(1, 1, Action::NewRender, 0.9), 0));
        s.on_search(found(2, "owl", 0.6), 1000);
        assert_eq!(s.on_decision(dec(2, 2, Action::NewRender, 0.9), 1000), Some(Outcome::Pending));
        assert_eq!(s.displayed().image_id.as_deref(), Some("owl"), "displayed shows the pending visual");
        assert!(s.tick(3999).is_empty());
        let out = s.tick(4000);
        assert!(matches!(&out[0].1, Outcome::Rendered(ev) if ev.image_id.as_deref() == Some("owl")));
    }

    #[test]
    fn newer_pending_replaces_older_and_keep_does_not_cancel() {
        let mut s = Stage::new(StageConfig::default());
        s.on_search(found(1, "eagle", 0.6), 0);
        s.on_decision(dec(1, 1, Action::NewRender, 0.9), 0);
        s.on_search(found(2, "owl", 0.6), 500);
        s.on_decision(dec(2, 2, Action::NewRender, 0.9), 500);
        s.on_search(found(3, "parrot", 0.6), 800);
        s.on_decision(dec(3, 3, Action::NewRender, 0.9), 800);
        s.on_search(found(4, "zebra", 0.6), 900);
        assert_eq!(s.on_decision(dec(4, 4, Action::NoChange, 0.9), 900), Some(Outcome::NoChange));
        let out = s.tick(4000);
        assert!(matches!(&out[0].1, Outcome::Rendered(ev) if ev.image_id.as_deref() == Some("parrot")));
    }

    #[test]
    fn update_may_replace_after_1_5s() {
        let mut s = Stage::new(StageConfig::default());
        s.on_search(found(1, "car", 0.6), 0);
        s.on_decision(dec(1, 1, Action::NewRender, 0.9), 0);
        s.on_search(found(2, "red-car", 0.6), 1600);
        let ev = rendered(s.on_decision(dec(2, 2, Action::Update, 0.8), 1600));
        assert_eq!(ev.kind, "update");
    }

    #[test]
    fn duplicates_and_stale_sequences_are_dropped() {
        let mut s = Stage::new(StageConfig::default());
        s.on_search(found(5, "eagle", 0.6), 0);
        s.on_decision(dec(5, 10, Action::NewRender, 0.9), 0);
        s.on_search(found(6, "eagle", 0.6), 5000);
        assert_eq!(s.on_decision(dec(6, 11, Action::NewRender, 0.9), 5000), Some(Outcome::Duplicate));
        s.on_search(found(4, "owl", 0.6), 5000);
        assert_eq!(s.on_decision(dec(4, 9, Action::NewRender, 0.9), 5000), Some(Outcome::Stale));
    }

    #[test]
    fn missing_branch_times_out() {
        let mut s = Stage::new(StageConfig::default());
        s.on_decision(dec(1, 1, Action::NewRender, 0.9), 0);
        assert!(s.tick(1000).is_empty());
        assert_eq!(s.tick(1001), vec![(1, Outcome::TimedOut)]);
        // A late partner now starts a fresh half and cannot render alone.
        assert_eq!(s.on_search(found(1, "eagle", 0.6), 1100), None);
    }

    #[test]
    fn clear_needs_higher_probability_and_an_image_on_screen() {
        let mut s = Stage::new(StageConfig::default());
        s.on_search(SearchOutcome { chunk_id: 1, best: None }, 0);
        assert_eq!(s.on_decision(dec(1, 1, Action::Clear, 0.9), 0), Some(Outcome::Duplicate));
        s.on_search(found(2, "eagle", 0.6), 0);
        s.on_decision(dec(2, 2, Action::NewRender, 0.9), 0);
        s.on_search(SearchOutcome { chunk_id: 3, best: None }, 5000);
        assert_eq!(s.on_decision(dec(3, 3, Action::Clear, 0.65), 5000), Some(Outcome::BelowProbability));
        s.on_search(SearchOutcome { chunk_id: 4, best: None }, 5000);
        let ev = rendered(s.on_decision(dec(4, 4, Action::Clear, 0.8), 5000));
        assert_eq!(ev.kind, "clear");
        assert_eq!(ev.url, None);
    }

    #[test]
    fn trigger_text_comes_from_chunk() {
        let mut s = Stage::new(StageConfig::default());
        s.on_chunk(&Chunk { id: 1, text: "our eagle mascot".into(), t_start_ms: 0, t_end_ms: 1, is_final: false });
        s.on_search(found(1, "eagle", 0.6), 0);
        s.on_decision(dec(1, 1, Action::NewRender, 0.9), 0);
        assert_eq!(s.displayed().trigger_text, "our eagle mascot");
    }
}
