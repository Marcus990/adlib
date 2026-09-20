//! Offline helpers for when Luna is unavailable: the presenter cues ("here's…", "take a look at…") and the
//! library subject a phrase names outright. (The remote phrase model that used to live here was removed with
//! the Luna refactor: Luna names the photo subject itself.)

/// Presentational cues: phrases a presenter uses when pointing the audience at something to look at.
/// Used by the offline fallback (no key / Luna down).
pub const CUES: &[&str] = &[
    "here's", "here is", "here are", "take a look", "look at", "have a look", "picture this", "imagine",
    "as you can see", "you can see", "let me show you", "i'll show you", "check out", "this is what",
    "this is our", "this is the", "that's what", "what it looks like", "looks like this", "show you",
];

/// Words from `text` after its last presentational cue (≤ `window` words), or None if no cue.
pub fn after_last_cue(text: &str, window: usize) -> Option<Vec<String>> {
    after_last(text, window, CUES)
}

fn after_last(text: &str, window: usize, cues: &[&str]) -> Option<Vec<String>> {
    let lower = text.to_lowercase().replace('’', "'");
    let pos = cues.iter().filter_map(|c| lower.rfind(c).map(|i| i + c.len())).max()?;
    Some(
        lower[pos..]
            .split(|c: char| !(c.is_alphanumeric() || c == '\''))
            .filter(|w| !w.is_empty())
            .take(window)
            .map(String::from)
            .collect(),
    )
}

/// The one library subject the speech names outright ("Penguins can't fly…" → "penguin"), or None when
/// there are none, several ("owls and penguins"), or the name is ambiguous ("a rose and a white rose" —
/// the bare "rose" is left alone). Whole words; a plural -s/-es on the last word counts.
pub fn named_subject(text: &str, vocab: &[String]) -> Option<String> {
    let words: Vec<String> = text
        .to_lowercase()
        .split(|c: char| !(c.is_alphanumeric() || c == '-' || c == '\''))
        .map(|w| w.trim_matches(|c| c == '\'' || c == '-').trim_end_matches("'s").to_string())
        .filter(|w| !w.is_empty())
        .collect();
    let same = |w: &str, t: &str| w == t || w.strip_suffix('s') == Some(t) || w.strip_suffix("es") == Some(t);
    let mut found: Vec<(String, usize, usize)> = vec![]; // (subject, start, len)
    let mut subjects: Vec<String> = vocab.iter().map(|c| c.split('(').next().unwrap_or(c).trim().to_lowercase()).filter(|s| s.len() >= 3).collect();
    subjects.sort_by_key(|s| std::cmp::Reverse(s.split_whitespace().count())); // longest first
    subjects.dedup();
    for s in &subjects {
        let toks: Vec<&str> = s.split_whitespace().collect();
        for i in 0..words.len().saturating_sub(toks.len() - 1) {
            let hit = toks.iter().enumerate().all(|(k, t)| if k + 1 == toks.len() { same(&words[i + k], t) } else { words[i + k] == *t });
            let covered = found.iter().any(|(_, st, n)| i < st + n && st < &(i + toks.len()));
            if hit && !covered {
                found.push((s.clone(), i, toks.len()));
            }
        }
    }
    let distinct: std::collections::BTreeSet<&str> = found.iter().map(|f| f.0.as_str()).collect();
    if distinct.len() != 1 {
        return None;
    }
    let subject = distinct.into_iter().next().unwrap().to_string();
    // "a rose and a white rose": the head noun appears on its own too → let the phrase model decide.
    let head = subject.split_whitespace().last().unwrap_or(&subject).to_string();
    let head_count = words.iter().filter(|w| same(w, &head)).count();
    (head_count <= found.len()).then_some(subject)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_subject_only_when_unambiguous() {
        let v: Vec<String> = ["penguin (animals)", "owl (animals)", "red rose (flowers)", "white rose (flowers)", "sunflower (flowers)",
            "flower (flowers)", "earth (nature)", "yin-yang (fun)", "nest (nature)"].iter().map(|s| s.to_string()).collect();
        assert_eq!(named_subject("Penguins can't fly, but they're incredible swimmers.", &v).as_deref(), Some("penguin"));
        assert_eq!(named_subject("Think about a sunflower in a field.", &v).as_deref(), Some("sunflower"));
        assert_eq!(named_subject("Finally, here's the planet Earth from space.", &v).as_deref(), Some("earth"));
        assert_eq!(named_subject("Owls and penguins.", &v), None, "two subjects");
        assert_eq!(named_subject("So let's do a side-by-side of a rose and a white rose.", &v), None, "bare 'rose' is ambiguous");
        assert_eq!(named_subject("And here is a red rose, actually, make that the white rose instead.", &v), None);
        assert_eq!(named_subject("Honestly, the flowers were great", &v).as_deref(), Some("flower"));
        assert_eq!(named_subject("Hangouts can't fly, but they're incredible swimmers.", &v), None, "no library word");
        assert_eq!(named_subject("Honestly I think so", &v), None, "'nest' inside a word doesn't count");
    }
}
