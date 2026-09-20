//! Logo / icon / flag lookup over the asset card's `icons/` folder. No embeddings: a port of the reference
//! implementation `assets-pipeline/icon_search.py` (same scores, same thresholds), because a brand or a symbol
//! is found by its NAME and aliases, not by what it looks like.
//!
//! Files: `lookup.json` = `{kind: {normalised string: id}}` (kind is `logo` | `icon` | `flag`; every title, key
//! and alias of a brand points at the same preferred file) and `manifest.json` (one entry per SVG).
//!
//! Pipeline, cheapest first: (1) every contiguous span of the query's words against the lookup, longest first;
//! (2) if nothing is strong, fuzzy: character-trigram Dice, a prefix bonus ("postgre") and edit distance ≤ 2
//! ("kubernets"); (3) for `icon`, token overlap against each icon's name and tags ("security lock").

use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use unicode_normalization::UnicodeNormalization;

/// Words that say what the query is, not which one: dropped before matching.
const FILLER: &[&str] = &["logo", "logos", "icon", "icons", "flag", "flags", "symbol", "image", "picture", "svg", "the", "a", "an", "of", "for", "with", "and"];
/// Confidence to pick automatically / to show a candidate at all.
pub const STRONG: f32 = 0.85;
pub const WEAK: f32 = 0.6;

/// One SVG in the library.
#[derive(Debug, Clone, Deserialize)]
pub struct Entry {
    pub id: String,
    pub kind: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub title: String,
    /// Draws in `currentColor` (all Lucide icons): recolour it for the theme.
    #[serde(default)]
    pub monochrome: bool,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Relative to `icons/`.
    pub file: String,
}

/// A ranked candidate.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub id: String,
    pub score: f32,
    /// `exact` | `tags` | `fuzzy`
    pub how: &'static str,
    pub title: String,
}

struct Table {
    by_slug: HashMap<String, String>,
    /// (slug, sorted trigram set, id) for the fuzzy pass.
    fuzzy: Vec<(String, Vec<u32>, String)>,
}

pub struct IconSearch {
    dir: PathBuf,
    tables: HashMap<String, Table>,
    manifest: HashMap<String, Entry>,
    /// Per icon: name tokens, and the words of its tags beyond the name's own.
    icon_words: Vec<(String, HashSet<String>, HashSet<String>)>,
}

/// The rule the build used to turn a title into a lookup key: lowercase, a few letter replacements, NFD, keep a-z0-9.
pub fn title_to_slug(s: &str) -> String {
    let mut lowered = String::new();
    for c in s.to_lowercase().chars() {
        match c {
            '+' => lowered.push_str("plus"),
            '.' => lowered.push_str("dot"),
            '&' => lowered.push_str("and"),
            'đ' => lowered.push('d'),
            'ħ' => lowered.push('h'),
            'ı' => lowered.push('i'),
            'ĸ' => lowered.push('k'),
            'ŀ' | 'ł' => lowered.push('l'),
            'ß' => lowered.push_str("ss"),
            'ŧ' => lowered.push('t'),
            c => lowered.push(c),
        }
    }
    lowered.nfd().filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit()).collect()
}

/// Character trigrams of `  s ` as a sorted set (slugs are ASCII, so bytes are characters).
fn trigrams(s: &str) -> Vec<u32> {
    let padded = format!("  {s} ");
    let b = padded.as_bytes();
    let mut v: Vec<u32> = b.windows(3).map(|w| (w[0] as u32) << 16 | (w[1] as u32) << 8 | w[2] as u32).collect();
    v.sort_unstable();
    v.dedup();
    v
}

fn shared(a: &[u32], b: &[u32]) -> usize {
    let (mut i, mut j, mut n) = (0, 0, 0);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                n += 1;
                i += 1;
                j += 1;
            }
        }
    }
    n
}

/// Optimal string alignment distance (insert, delete, substitute, transpose).
fn edit_distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut prev2: Vec<usize> = vec![];
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i; b.len() + 1];
        for j in 1..=b.len() {
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + usize::from(a[i - 1] != b[j - 1]));
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                cur[j] = cur[j].min(prev2[j - 2] + 1);
            }
        }
        prev2 = prev;
        prev = cur;
    }
    prev[b.len()]
}

/// Words of a query: `[\w+.&#]+` runs, lowercase.
fn words_of(q: &str, keep: impl Fn(char) -> bool) -> Vec<String> {
    let mut out = vec![];
    let mut cur = String::new();
    for c in q.to_lowercase().chars() {
        if keep(c) {
            cur.push(c);
        } else if !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn round2(x: f32) -> f32 {
    (x * 100.0).round() / 100.0
}

impl IconSearch {
    /// `icons_dir` is the card's `icons/` folder.
    pub fn load(icons_dir: &Path) -> Result<Self> {
        let lookup: HashMap<String, HashMap<String, String>> =
            serde_json::from_str(&std::fs::read_to_string(icons_dir.join("lookup.json")).context("icons/lookup.json")?)?;
        let entries: Vec<Entry> = serde_json::from_str(&std::fs::read_to_string(icons_dir.join("manifest.json")).context("icons/manifest.json")?)?;
        let manifest: HashMap<String, Entry> = entries.into_iter().map(|e| (e.id.clone(), e)).collect();
        let tables = lookup
            .into_iter()
            .map(|(kind, t)| {
                let fuzzy = t.iter().map(|(s, id)| (s.clone(), trigrams(s), id.clone())).collect();
                (kind, Table { by_slug: t, fuzzy })
            })
            .collect();
        let icon_words = manifest
            .values()
            .filter(|e| e.kind == "icon")
            .map(|e| {
                let name_tokens: Vec<&str> = e.name.split('-').collect();
                let name_w: HashSet<String> = name_tokens.iter().map(|t| t.to_string()).collect();
                let tag_w: HashSet<String> = e.tags.iter().skip(name_tokens.len()).flat_map(|t| t.to_lowercase().split_whitespace().map(String::from).collect::<Vec<_>>()).collect();
                (e.id.clone(), name_w, tag_w)
            })
            .collect();
        Ok(Self { dir: icons_dir.to_path_buf(), tables, manifest, icon_words })
    }

    pub fn entry(&self, id: &str) -> Option<&Entry> {
        self.manifest.get(id)
    }

    pub fn path_of(&self, id: &str) -> Option<PathBuf> {
        self.manifest.get(id).map(|e| self.dir.join(&e.file))
    }

    /// Ranked candidates (best first, at most `k`) for `query` among `kind` (`logo` | `icon` | `flag`).
    pub fn search(&self, kind: &str, query: &str, k: usize) -> Vec<Hit> {
        let is_word = |c: char| c.is_alphanumeric() || matches!(c, '_' | '+' | '.' | '&' | '#');
        let mut words: Vec<String> = words_of(query, is_word).into_iter().filter(|w| !FILLER.contains(&w.as_str())).collect();
        if words.is_empty() {
            words = words_of(query, |c| c.is_alphanumeric() || c == '_');
        }
        let mut hits: HashMap<String, (f32, &'static str)> = HashMap::new();
        fn add(hits: &mut HashMap<String, (f32, &'static str)>, id: &str, score: f32, how: &'static str) {
            if score > hits.get(id).map_or(0.0, |h| h.0) {
                hits.insert(id.to_string(), (score, how));
            }
        }
        let empty = Table { by_slug: HashMap::new(), fuzzy: vec![] };
        let table = self.tables.get(kind).unwrap_or(&empty);
        let total = words.iter().map(|w| w.chars().count()).sum::<usize>().max(1) as f32;
        // 1. exact spans, longest first
        for n in (1..=words.len()).rev() {
            for i in 0..=words.len() - n {
                let span = &words[i..i + n];
                if let Some(id) = table.by_slug.get(&title_to_slug(&span.join(" "))) {
                    add(&mut hits, id, 0.7 + 0.3 * span.iter().map(|w| w.chars().count()).sum::<usize>() as f32 / total, "exact");
                }
            }
        }
        // 3. concept search over icon names and tags
        if kind == "icon" && !words.is_empty() {
            let qw: HashSet<&str> = words.iter().map(|w| w.as_str()).collect();
            for (id, name_w, tag_w) in &self.icon_words {
                let covered = qw.iter().filter(|w| name_w.contains(**w) || tag_w.contains(**w)).count();
                if covered > 0 {
                    let in_name = qw.iter().filter(|w| name_w.contains(**w)).count();
                    add(&mut hits, id, (0.2 + 0.4 * covered as f32 / qw.len() as f32 + 0.1 * in_name as f32 / qw.len() as f32).min(0.8), "tags");
                }
            }
        }
        // 2. fuzzy fallback, only when nothing is strong yet
        if !hits.values().any(|h| h.0 >= STRONG) {
            let q = title_to_slug(&words.join(""));
            let qt = trigrams(&q);
            let qlen = q.chars().count();
            for (s, st, id) in &table.fuzzy {
                let slen = s.chars().count();
                let mut sim = 2.0 * shared(&qt, st) as f32 / (qt.len() + st.len()) as f32;
                if s.starts_with(&q) && qlen >= 3 {
                    sim = sim.max(0.7 + 0.2 * qlen as f32 / slen as f32); // partial name: "postgre"
                } else if slen.abs_diff(qlen) <= 2 && qlen >= 4 && (s.chars().next() == q.chars().next() || s.chars().last() == q.chars().last()) {
                    let d = edit_distance(&q, s);
                    if d <= 2 {
                        sim = sim.max(1.0 - d as f32 / slen.max(qlen) as f32 + 0.02); // typos: "kubernets"
                    }
                }
                if sim >= WEAK {
                    add(&mut hits, id, sim * 0.95, "fuzzy");
                }
            }
        }
        let mut ranked: Vec<(String, (f32, &'static str))> = hits.into_iter().collect();
        ranked.sort_by(|a, b| b.1 .0.partial_cmp(&a.1 .0).unwrap().then(a.0.cmp(&b.0)));
        ranked
            .into_iter()
            .take(k)
            .map(|(id, (score, how))| Hit { title: self.manifest.get(&id).map(|e| e.title.clone()).unwrap_or_default(), id, score: round2(score), how })
            .collect()
    }

    /// The candidate to show, if one is clearly right. Icons: the top hit ≥ [`STRONG`], or ≥ [`WEAK`] and at least
    /// 0.15 ahead of the runner-up. Logos are stricter (see [`choose`]). Otherwise `None` (the ranked list is still returned).
    pub fn pick(&self, kind: &str, query: &str) -> (Option<Hit>, Vec<Hit>) {
        let ranked = self.search(kind, query, 5);
        (choose(&ranked, kind == "logo"), ranked)
    }

    /// The picture for a diagram node or chart point, or `None`. Precision over recall: a wrong picture is worse than none.
    /// 1. `logo`, the product the model named: a logo by exact name / alias, or a strong fuzzy match ([`IconSearch::pick`]);
    /// 2. a generic icon whose whole name is EXACTLY the model's `concept` word, or (when `label_icon`) the node's whole label.
    /// A partial or tag-only match never qualifies ("queue" only matches `columns-2` by tag; a bare "Message queue" label is no icon).
    pub fn picture_for(&self, logo: Option<&str>, concept: Option<&str>, label: &str, label_icon: bool) -> Option<Hit> {
        if let Some(brand) = logo.map(str::trim).filter(|b| !b.is_empty()) {
            if let (Some(hit), _) = self.pick("logo", brand) {
                return Some(hit);
            }
        }
        let whole = |q: &str| self.search("icon", q, 1).into_iter().next().filter(|h| h.how == "exact" && h.score >= 0.99);
        concept.map(str::trim).filter(|c| !c.is_empty()).and_then(whole).or_else(|| if label_icon { whole(label) } else { None })
    }

    /// Several phrasings of one thing across several kinds ("teamwork", then the synonyms the model offered), best
    /// score per id. The first phrasing gets a small edge over the later ones so an exact hit on the concept itself wins ties.
    pub fn resolve(&self, kinds: &[&str], queries: &[String]) -> (Option<Hit>, Vec<Hit>) {
        let mut best: HashMap<String, Hit> = HashMap::new();
        for (i, q) in queries.iter().enumerate() {
            for kind in kinds {
                for mut h in self.search(kind, q, 5) {
                    h.score = round2((h.score - 0.02 * i.min(3) as f32).max(0.0));
                    if best.get(&h.id).map_or(true, |b| h.score > b.score) {
                        best.insert(h.id.clone(), h);
                    }
                }
            }
        }
        let mut ranked: Vec<Hit> = best.into_values().collect();
        ranked.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap().then(a.id.cmp(&b.id)));
        ranked.truncate(5);
        (choose(&ranked, kinds == ["logo"]), ranked)
    }
}

/// Which candidate, if any, is safe to put on screen without asking anyone.
/// - Icons and flags: the top hit ≥ [`STRONG`], or ≥ [`WEAK`] and ≥ 0.15 ahead of the runner-up (a slightly loose
///   symbol is still a fair symbol).
/// - Logos (`strict`): only an exact name / alias match, or a fuzzy match ≥ [`STRONG`] (a typo of a long name). A lone weak
///   fuzzy hit is how "Hooli" became a hoodie; a wrong logo is worse than a name card.
fn choose(ranked: &[Hit], strict: bool) -> Option<Hit> {
    let top = ranked.first()?;
    if strict {
        return (top.how == "exact" || top.score >= STRONG).then(|| top.clone());
    }
    let next = ranked.get(1).map_or(0.0, |h| h.score);
    (top.score >= STRONG || (top.score >= WEAK && top.score - next >= 0.15)).then(|| top.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A small library in a temp dir: brands (several spellings each), lucide-style icons with tags, a flag.
    fn library() -> (PathBuf, IconSearch) {
        let dir = std::env::temp_dir().join(format!("ls-icons-test-{}-{:?}", std::process::id(), std::thread::current().id()));
        std::fs::create_dir_all(&dir).unwrap();
        let e = |id: &str, kind: &str, name: &str, title: &str, tags: &[&str]| {
            serde_json::json!({"id": id, "kind": kind, "name": name, "title": title, "tags": tags, "file": format!("{name}.svg"), "monochrome": kind == "icon"})
        };
        let manifest = serde_json::json!([
            e("logos:google-icon", "logo", "google-icon", "Google", &["google", "icon"]),
            e("logos:google-cloud", "logo", "google-cloud", "Google Cloud", &["google", "cloud"]),
            e("logos:kubernetes", "logo", "kubernetes", "Kubernetes", &["kubernetes"]),
            e("logos:postgresql", "logo", "postgresql", "PostgreSQL", &["postgresql"]),
            e("logos:hoodie", "logo", "hoodie", "Hoodie", &["hoodie"]),
            e("lucide:database", "icon", "database", "Database", &["database", "storage", "memory", "servers"]),
            e("lucide:users", "icon", "users", "Users", &["users", "group", "people", "team", "community"]),
            e("lucide:shield", "icon", "shield", "Shield", &["shield", "security", "safe", "protection"]),
            e("circle-flags:ca", "flag", "ca", "Canada", &["canada"]),
        ]);
        let lookup = serde_json::json!({
            "logo": {"google": "logos:google-icon", "googleicon": "logos:google-icon", "googlelogo": "logos:google-icon", "googlecloud": "logos:google-cloud",
                     "googlecloudplatform": "logos:google-cloud", "gcp": "logos:google-cloud", "postgres": "logos:postgresql", "kubernetes": "logos:kubernetes", "k8s": "logos:kubernetes",
                     "postgresql": "logos:postgresql", "hoodie": "logos:hoodie"},
            "icon": {"database": "lucide:database", "users": "lucide:users", "shield": "lucide:shield"},
            "flag": {"canada": "circle-flags:ca", "flagofcanada": "circle-flags:ca"}
        });
        std::fs::write(dir.join("manifest.json"), manifest.to_string()).unwrap();
        std::fs::write(dir.join("lookup.json"), lookup.to_string()).unwrap();
        let s = IconSearch::load(&dir).unwrap();
        (dir, s)
    }

    #[test]
    fn slugs_follow_the_build_rule() {
        assert_eq!(title_to_slug("Google Cloud Platform"), "googlecloudplatform");
        assert_eq!(title_to_slug("C++"), "cplusplus");
        assert_eq!(title_to_slug("Node.js"), "nodedotjs");
        assert_eq!(title_to_slug("AT&T"), "atandt");
        assert_eq!(title_to_slug("Nestlé"), "nestle", "accents fold via NFD");
        assert_eq!(title_to_slug("Škoda ß"), "skodass");
    }

    #[test]
    fn a_company_named_in_a_sentence_finds_its_logo() {
        let (_d, s) = library();
        for q in ["Google", "google logo", "the logo of the company called Google", "GOOGLE!"] {
            let (pick, ranked) = s.pick("logo", q);
            assert_eq!(pick.map(|h| h.id).as_deref(), Some("logos:google-icon"), "{q}: {ranked:?}");
        }
        // the longer span wins over its first word
        assert_eq!(s.pick("logo", "the Google Cloud Platform logo").0.unwrap().id, "logos:google-cloud");
        assert_eq!(s.pick("logo", "GCP").0.unwrap().id, "logos:google-cloud", "aliases point at the same file");
    }

    #[test]
    fn a_typo_of_a_long_name_still_finds_it() {
        let (_d, s) = library();
        assert_eq!(s.pick("logo", "kubernets").0.unwrap().id, "logos:kubernetes");
        assert_eq!(s.search("logo", "kubernets", 3)[0].how, "fuzzy");
    }

    #[test]
    fn a_weak_lookalike_is_a_candidate_but_never_shown_as_a_logo() {
        let (_d, s) = library();
        // "Hooli" is not in the library; "Hoodie" is a lone 0.65 fuzzy hit. Ranked, but not picked for a logo.
        let (pick, ranked) = s.pick("logo", "Hooli");
        assert!(pick.is_none(), "{ranked:?}");
        assert_eq!(ranked[0].id, "logos:hoodie");
        // a partial name ("postgre") is also only a candidate: a person names a whole brand
        let (pick, ranked) = s.pick("logo", "postgre");
        assert!(pick.is_none() && ranked[0].id == "logos:postgresql", "{ranked:?}");
        // the multi-query path (what the pipeline calls) applies the same rule
        assert!(s.resolve(&["logo"], &["Hooli".to_string()]).0.is_none());
    }

    #[test]
    fn nonsense_and_unknown_brands_find_nothing() {
        let (_d, s) = library();
        assert!(s.pick("logo", "zzqxv").0.is_none());
        assert!(s.pick("logo", "Hooli").0.is_none(), "not in the library → the caller shows a name card");
        assert!(s.pick("logo", "").0.is_none());
    }

    #[test]
    fn concept_icons_match_by_name_and_tags_and_the_models_synonyms_widen_it() {
        let (_d, s) = library();
        assert_eq!(s.pick("icon", "database").0.unwrap().id, "lucide:database");
        // "security" is only a tag of the shield icon
        assert_eq!(s.pick("icon", "security").0.unwrap().id, "lucide:shield");
        // "teamwork" is on no icon: nothing…
        assert!(s.pick("icon", "teamwork").0.is_none());
        // …until the model offers synonyms ("users" is an exact icon name)
        let q: Vec<String> = ["teamwork", "users", "group", "people"].iter().map(|s| s.to_string()).collect();
        let (pick, ranked) = s.resolve(&["icon", "flag"], &q);
        assert_eq!(pick.map(|h| h.id).as_deref(), Some("lucide:users"), "{ranked:?}");
        // a flag by country name, through the same call
        let (pick, _) = s.resolve(&["icon", "flag"], &["flag of Canada".to_string()]);
        assert_eq!(pick.unwrap().id, "circle-flags:ca");
    }

    #[test]
    fn a_node_gets_a_logo_for_a_named_product_or_a_generic_icon_by_exact_name_and_otherwise_nothing() {
        let (_d, s) = library();
        let id = |h: Option<Hit>| h.map(|h| h.id);
        // the product the model named wins over the generic word
        assert_eq!(id(s.picture_for(Some("Postgres"), Some("database"), "Postgres", true)).as_deref(), Some("logos:postgresql"));
        // a generic node: the model's icon word, exact
        assert_eq!(id(s.picture_for(None, Some("database"), "Orders store", false)).as_deref(), Some("lucide:database"));
        assert_eq!(id(s.picture_for(None, None, "Users", true)).as_deref(), Some("lucide:users"), "a label that IS an icon name");
        assert!(s.picture_for(None, None, "Users", false).is_none(), "charts do not take an icon from the label");
        // weak or partial matches are no picture: a tag hit ("security" only tags the shield icon), a label that merely contains a name
        assert!(s.picture_for(None, Some("security"), "Auth", true).is_none());
        assert!(s.picture_for(None, None, "Database service", true).is_none());
        assert!(s.picture_for(None, Some("teamwork"), "Team", true).is_none());
        // a brand that is not in the library: nothing (never a lookalike), and a generic word given as a brand is nothing
        assert!(s.picture_for(Some("Hooli"), None, "Hooli", false).is_none());
        assert!(s.picture_for(Some("database"), None, "Database", false).is_none(), "logo hints are for products, and 'database' is no logo");
    }

    #[test]
    fn edit_distance_counts_transpositions_once() {
        assert_eq!(edit_distance("japn", "japan"), 1);
        assert_eq!(edit_distance("teh", "the"), 1);
        assert_eq!(edit_distance("abc", "abc"), 0);
    }

    #[test]
    fn paths_and_entries_resolve() {
        let (d, s) = library();
        assert_eq!(s.path_of("lucide:database").unwrap(), d.join("database.svg"));
        assert!(s.entry("lucide:database").unwrap().monochrome && !s.entry("logos:google-icon").unwrap().monochrome);
        assert!(s.path_of("nope").is_none());
    }
}
