"""Reference implementation of icon/logo search over assets/icons/lookup.json + manifest.json.

Port this to Rust. Pipeline, cheapest first:
  1. exact lookup of every contiguous span of the query's words, longest first
     ("the google cloud platform logo" -> 'googlecloudplatform');
  2. fuzzy: character-trigram Dice similarity against every lookup string
     (typos, partial names: 'kubernets', 'postgre');
  3. for generic icons, token overlap against each icon's name + Lucide tags
     ('growth chart' -> trending-up), since many icons share a tag.
Returns ranked candidates with a confidence in [0, 1]; the caller can take the top hit
when confidence is high and hand the top few to the model when it is not.

Usage: LIBRARY_DIR=... python assets-pipeline/icon_search.py "kind" "query"
"""

import json
import os
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from build_icon_library import title_to_slug  # noqa: E402  (same normalization as the build)

ICONS_DIR = Path(os.environ.get("LIBRARY_DIR") or Path(__file__).resolve().parent.parent / "assets") / "icons"
FILLER = {"logo", "logos", "icon", "icons", "flag", "flags", "symbol", "image", "picture", "svg", "the", "a", "an", "of", "for", "with", "and"}
STRONG, WEAK = 0.85, 0.6  # confidence to auto-pick / to show candidates at all


def edit_distance(a, b):
    """Optimal string alignment distance (insert/delete/substitute/transpose)."""
    prev2, prev = None, list(range(len(b) + 1))
    for i in range(1, len(a) + 1):
        cur = [i] + [0] * len(b)
        for j in range(1, len(b) + 1):
            cur[j] = min(prev[j] + 1, cur[j - 1] + 1, prev[j - 1] + (a[i - 1] != b[j - 1]))
            if i > 1 and j > 1 and a[i - 1] == b[j - 2] and a[i - 2] == b[j - 1]:
                cur[j] = min(cur[j], prev2[j - 2] + 1)
        prev2, prev = prev, cur
    return prev[-1]


def trigrams(s):
    s = f"  {s} "
    return {s[i:i + 3] for i in range(len(s) - 2)}


class IconSearch:
    def __init__(self, icons_dir=ICONS_DIR):
        self.lookup = json.load(open(icons_dir / "lookup.json"))
        self.manifest = {e["id"]: e for e in json.load(open(icons_dir / "manifest.json"))}
        self.tri = {k: {s: trigrams(s) for s in t} for k, t in self.lookup.items()}
        self.icon_words = {
            i: ({w for t in e["name"].split("-") for w in [t]}, {w for tag in e["tags"][len(e["name"].split("-")):] for w in tag.lower().split()})
            for i, e in self.manifest.items() if e["kind"] == "icon"
        }

    def search(self, kind, query, k=5):
        words = [w for w in re.findall(r"[\w+.&#]+", query.lower()) if w not in FILLER] or re.findall(r"\w+", query.lower())
        hits = {}

        def add(id_, score, how):
            if score > hits.get(id_, (0, ""))[0]:
                hits[id_] = (score, how)

        table = self.lookup.get(kind, {})
        total = sum(len(w) for w in words) or 1
        for n in range(len(words), 0, -1):  # 1. exact spans, longest first
            for i in range(len(words) - n + 1):
                span = words[i:i + n]
                if (id_ := table.get(title_to_slug(" ".join(span)))) :
                    add(id_, 0.7 + 0.3 * sum(len(w) for w in span) / total, "exact")
        if kind == "icon":  # 3. concept search over names + tags
            qw = set(words)
            for id_, (name_w, tag_w) in self.icon_words.items():
                if covered := len(qw & (name_w | tag_w)):
                    add(id_, min(0.8, 0.2 + 0.4 * covered / len(qw) + 0.1 * len(qw & name_w) / len(qw)), "tags")
        if not any(s >= STRONG for s, _ in hits.values()):  # 2. fuzzy fallback
            q = title_to_slug("".join(words))
            qt = trigrams(q)
            for s, st in self.tri.get(kind, {}).items():
                sim = 2 * len(qt & st) / (len(qt) + len(st))
                if s.startswith(q) and len(q) >= 3:
                    sim = max(sim, 0.7 + 0.2 * len(q) / len(s))  # partial name: 'postgre'
                elif abs(len(s) - len(q)) <= 2 and len(q) >= 4 and (s[0] == q[0] or s[-1] == q[-1]) and (d := edit_distance(q, s)) <= 2:
                    sim = max(sim, 1 - d / max(len(q), len(s)) + 0.02)  # typos: 'japn', 'kubernets'
                if sim >= WEAK:
                    add(table[s], sim * 0.95, "fuzzy")
        ranked = sorted(hits.items(), key=lambda kv: (-kv[1][0], kv[0]))[:k]
        return [(id_, round(score, 2), how, self.manifest[id_]["title"]) for id_, (score, how) in ranked]


    def pick(self, kind, query, k=5):
        """(id or None, candidates). Auto-pick when confident: top >= STRONG, or top >= WEAK and
        clearly ahead of the runner-up. Otherwise return None and let the model choose from candidates."""
        ranked = self.search(kind, query, k)
        if ranked and (ranked[0][1] >= STRONG or (ranked[0][1] >= WEAK and ranked[0][1] - (ranked[1][1] if len(ranked) > 1 else 0) >= 0.15)):
            return ranked[0][0], ranked
        return None, ranked


if __name__ == "__main__":
    for row in IconSearch().search(sys.argv[1], sys.argv[2]):
        print(row)
