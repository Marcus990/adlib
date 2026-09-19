"""Build the local icon + logo library (transparent-background SVGs) from Iconify.

Writes assets/icons/<set>/<name>.svg plus assets/icons/manifest.json and
README.md. Kept separate from assets/manifest.json + embeddings.npy, which are
index-aligned with the photo library and must not be appended to by this script.

Usage: LIBRARY_DIR=/Volumes/NO\\ NAME/assets python assets-pipeline/build_icon_library.py
Safe to re-run: existing SVGs are skipped, the manifest is rebuilt from scratch.
"""

import collections
import json
import os
import re
import unicodedata
from pathlib import Path

import requests

ASSETS_DIR = Path(os.environ.get("LIBRARY_DIR") or Path(__file__).resolve().parent.parent / "assets")
ICONS_DIR = ASSETS_DIR / "icons"
MANIFEST_PATH = ICONS_DIR / "manifest.json"
LOOKUP_PATH = ICONS_DIR / "lookup.json"
LUCIDE_TAGS_URL = "https://lucide.dev/api/tags"

SET_URL = "https://raw.githubusercontent.com/iconify/icon-sets/master/json/{prefix}.json"
SIMPLE_ICONS_DATA_URL = "https://raw.githubusercontent.com/simple-icons/simple-icons/develop/data/simple-icons.json"

# (prefix, kind, license, monochrome). Monochrome sets draw in currentColor
# (simple-icons gets its official brand color baked in instead).
SETS = [
    ("lucide", "icon", "ISC", True),
    ("logos", "logo", "CC0-1.0", False),
    ("thesvg-color", "logo", "MIT", False),
    ("simple-icons", "logo", "CC0-1.0", True),
    ("circle-flags", "flag", "MIT", False),
]

RENDER_SIDE = 512  # px on the longest side; viewBox keeps it scalable regardless
NUM = r"-?[\d.]+"
SLUG_REPLACEMENTS = {"+": "plus", ".": "dot", "&": "and", "đ": "d", "ħ": "h", "ı": "i", "ĸ": "k", "ŀ": "l", "ł": "l", "ß": "ss", "ŧ": "t"}


def title_to_slug(title):
    """Same rule simple-icons uses to turn a brand title into its slug."""
    s = "".join(SLUG_REPLACEMENTS.get(c, c) for c in title.lower())
    return re.sub(r"[^a-z0-9]", "", unicodedata.normalize("NFD", s))


def brand_data():
    """slug -> {title, hex, aka} from simple-icons' own metadata."""
    data = requests.get(SIMPLE_ICONS_DATA_URL, timeout=60).json()
    return {
        entry.get("slug") or title_to_slug(entry["title"]): {
            "title": entry["title"],
            "hex": "#" + entry["hex"],
            "aka": entry.get("aliases", {}).get("aka", []),
        }
        for entry in data
    }


def has_white_background(body, left, top, width, height):
    """True if the first shape is a white rect covering the whole canvas."""
    m = re.search(r"<rect\b[^>]*>", body[:300])
    if not m:
        return False
    attrs = dict(re.findall(r'([\w-]+)="([^"]*)"', m.group(0)))
    fill = attrs.get("fill", "").lower().replace(" ", "")
    if fill not in ("#fff", "#ffffff", "white", "rgb(255,255,255)"):
        return False
    try:
        x, y = float(attrs.get("x", left)), float(attrs.get("y", top))
        w, h = float(attrs["width"].rstrip("%")), float(attrs["height"].rstrip("%"))
    except (KeyError, ValueError):
        return False
    return abs(x - left) < 0.01 * width and abs(y - top) < 0.01 * height and w >= 0.98 * width and h >= 0.98 * height


def to_svg(body, left, top, width, height, fill=None):
    scale = RENDER_SIDE / max(width, height)
    if fill:
        body = f'<g fill="{fill}">{body}</g>'
    return (
        '<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" '
        f'viewBox="{left:g} {top:g} {width:g} {height:g}" width="{round(width * scale)}" height="{round(height * scale)}">'
        f"{body}</svg>\n"
    )


def build_set(prefix, kind, license_id, mono, brands):
    data = requests.get(SET_URL.format(prefix=prefix), timeout=120).json()
    set_w, set_h = data.get("width", 16), data.get("height", 16)
    aliases = {}
    for alias, spec in data.get("aliases", {}).items():
        aliases.setdefault(spec["parent"], []).append(alias)

    out_dir = ICONS_DIR / prefix
    out_dir.mkdir(parents=True, exist_ok=True)
    entries, skipped_bg, unmatched_color = [], 0, 0
    for name, icon in sorted(data["icons"].items()):
        left, top = icon.get("left", 0), icon.get("top", 0)
        width, height = icon.get("width", set_w), icon.get("height", set_h)
        body = icon["body"]
        if width <= 0 or height <= 0:  # e.g. thesvg-color:logitech-g
            continue
        if has_white_background(body, left, top, width, height):
            skipped_bg += 1
            continue
        fill, hex_color = None, None
        if prefix == "simple-icons":
            hex_color = brands.get(name, {}).get("hex")
            if hex_color is None:
                unmatched_color += 1
            fill = hex_color or "#000000"
        path = out_dir / f"{name}.svg"
        if not path.exists():
            path.write_text(to_svg(body, left, top, width, height, fill))
        entries.append(
            {
                "id": f"{prefix}:{name}",
                "set": prefix,
                "name": name,
                "kind": kind,
                "monochrome": mono and prefix != "simple-icons",
                "color": hex_color,
                "aliases": sorted(aliases.get(name, [])),
                "tags": name.split("-"),
                "license": license_id,
                "file": f"{prefix}/{name}.svg",
                "aspect": round(width / height, 3),
            }
        )
    print(f"{prefix:14} {len(entries):5} icons  (skipped {skipped_bg} white-background, {unmatched_color} without brand color)", flush=True)
    return entries


VARIANT_TOKENS = {"icon", "dark", "light", "wordmark", "logo"}
THEME_TOKENS = {"dark", "light"}
SET_PRIORITY = {"logos": 0, "thesvg-color": 1, "simple-icons": 2}

CUSTOM_DIR = Path(__file__).resolve().parent / "custom_icons"  # hand-supplied SVGs -> icons/custom/
ALIASES_PATH = Path(__file__).resolve().parent / "icon_aliases.json"  # {brand key: [alternate names]}
COUNTRIES_URL = "https://raw.githubusercontent.com/mledoze/countries/master/dist/countries.json"

# The same brand often has different names in different sets (simple-icons 'amazons3' vs
# logos 'aws-s3'). PREFIX_MERGES catches the systematic cases; KEY_MERGES is curated and
# skips generic-looking ones (googleplay -> play, googlelens -> lens) on purpose.
PREFIX_MERGES = {"amazon": "aws", "apache": ""}
KEY_MERGES = {
    "amazonwebservices": "aws", "microsoftazure": "azure", "microsoftbing": "bing", "microsoftdotnet": "dotnet",
    "microsoftedge": "edge", "microsoftpowerbi": "powerbi", "microsoftwindows": "windows", "googlechrome": "chrome",
    "googlegemini": "gemini", "googlecloudplatform": "googlecloud", "dynamodb": "awsdynamodb", "d3dotjs": "d3", "expressdotjs": "express", "mui": "materialui", "opentf": "opentofu", "tuta": "tutanota", "dependencycheck": "owaspdependencycheck", "googlegmail": "gmail", "googleadmob": "admob", "vuedotjs": "vue",
}
TITLE_OVERRIDES = {"openai": "OpenAI", "aws": "AWS"}
EXTRA_ALIASES = {"aws": ["Amazon Web Services"]}

# Flags that are not in the countries dataset, and informal names people use for countries.
FLAG_NAMES = {"eu": "European Union", "un": "United Nations", "gb-eng": "England", "gb-sct": "Scotland", "gb-wls": "Wales", "gb-nir": "Northern Ireland"}
EXTRA_FLAG_ALIASES = {
    "us": ["America"], "gb": ["UK", "Britain", "Great Britain"], "nl": ["Holland"], "ae": ["UAE"], "kr": ["Korea"],
    "cz": ["Czech Republic"], "mm": ["Burma"], "tr": ["Turkiye"], "ru": ["Russian Federation"], "sz": ["Swaziland"],
}


def brand_tokens(name):
    """Name tokens minus trailing variant markers: 'openai-icon' -> ['openai']."""
    tokens = name.split("-")
    while len(tokens) > 1 and tokens[-1] in VARIANT_TOKENS:
        tokens.pop()
    return tokens


def preference_rank(entry):
    """Lower is better: not theme-specific, then the mark over the wordmark, then full color over mono."""
    tokens = entry["name"].split("-")
    themed = len(tokens) > 1 and tokens[-1] in THEME_TOKENS
    is_mark = len(tokens) > 1 and tokens[-1] == "icon"
    return (themed, not is_mark, SET_PRIORITY.get(entry["set"], 9), entry["name"])


def resolve_merges(keys):
    merges = {k: v for k, v in KEY_MERGES.items() if k in keys}
    for k in keys:
        for prefix, replacement in PREFIX_MERGES.items():
            target = replacement + k[len(prefix):]
            if k.startswith(prefix) and len(k) > len(prefix) and target in keys:
                merges[k] = target
        if "dotjs" in k and k.replace("dotjs", "js") in keys:  # simple-icons 'nodedotjs' vs logos 'nodejs'
            merges[k] = k.replace("dotjs", "js")
    return merges


QUERY_WORDS = {"logo": ("logo", "icon"), "icon": ("icon",), "flag": ("flag",)}
QUERY_PREFIXES = {"flag": "flagof"}  # 'flag of germany'


def build_custom():
    out_dir = ICONS_DIR / "custom"
    out_dir.mkdir(parents=True, exist_ok=True)
    entries = []
    for src in sorted(CUSTOM_DIR.glob("*.svg")):
        text = src.read_text()
        (out_dir / src.name).write_text(text)
        _, _, w, h = map(float, re.search(r'viewBox="([^"]+)"', text).group(1).split())
        entries.append(
            {
                "id": f"custom:{src.stem}", "set": "custom", "name": src.stem, "kind": "logo",
                "monochrome": "currentColor" in text, "color": None, "aliases": [], "tags": src.stem.split("-"),
                "license": "user-supplied (trademark of its owner)", "file": f"custom/{src.name}", "aspect": round(w / h, 3),
            }
        )
    print(f"{'custom':14} {len(entries):5} icons", flush=True)
    return entries


def annotate(entries, brands, extra_aliases):
    """Add key / title / preferred to icons and logos. Entries sharing a key (across sets,
    light/dark/icon variants and merged names) are one brand; exactly one logo per brand is preferred."""
    natural = {e["id"]: title_to_slug("".join(brand_tokens(e["name"]))) for e in entries if e["kind"] != "flag"}
    merges = resolve_merges(set(natural.values()))
    print(f"merged {len(merges)} cross-set brand names", flush=True)
    groups = {}
    for e in entries:
        if e["kind"] == "flag":
            continue
        e["key"] = merges.get(natural[e["id"]], natural[e["id"]])
        groups.setdefault((e["kind"], e["key"]), []).append(e)
    missing = sorted(set(extra_aliases) - {key for _, key in groups})
    if missing:
        raise SystemExit(f"icon_aliases.json has keys that match no brand: {missing}")
    for (kind, key), members in groups.items():
        best = min(members, key=preference_rank) if kind == "logo" else None
        found = [brands[o] for o in sorted({natural[e["id"]] for e in members}) if o in brands]
        derived = " ".join(t.capitalize() for t in brand_tokens((best or members[0])["name"]))
        title = TITLE_OVERRIDES.get(key) or (found[0]["title"] if found else derived)
        merged_names = {natural[e["id"]] for e in members} - {key}
        aliases = {a for b in found for a in b["aka"]} | set(EXTRA_ALIASES.get(key, ())) | set(extra_aliases.get(key, ())) | merged_names
        for e in members:
            e["title"] = title
            e["preferred"] = e is best if kind == "logo" else True
            e["aliases"] = sorted(set(e["aliases"]) | aliases)
    return entries


STRIP_PREFIXES = ("amazon ", "aws ", "apache ", "azure ", "microsoft ", "ibm ")


def title_variants(title):
    """Shorter names people actually say: 'Amazon S3' -> 'S3', 'Node.js' -> 'Node'."""
    out = {title[len(p):] for p in STRIP_PREFIXES if title.lower().startswith(p)}
    bare = re.sub(r"[ .]?js$", "", title, flags=re.I)
    if bare != title:
        out.add(bare)
    return out


def add_derived_aliases(entries):
    """Add title variants as aliases only when no other brand already answers to that
    string and no two brands derive the same one, so a derived alias can never hijack."""
    by_group = {}
    for e in entries:
        if e["kind"] == "logo":
            by_group.setdefault(e["key"], []).append(e)
    owners = {}  # normalized string -> brand keys already using it
    for key, members in by_group.items():
        for text in {members[0]["title"], key, *members[0]["aliases"]}:
            owners.setdefault(title_to_slug(text), set()).add(key)
    derived = {}
    for key, members in by_group.items():
        for text in title_variants(members[0]["title"]):
            derived.setdefault(title_to_slug(text), []).append((key, text))
    added = 0
    for n, claims in derived.items():
        if len(n) >= 3 and len(claims) == 1 and n not in owners:
            key, text = claims[0]
            for e in by_group[key]:
                e["aliases"] = sorted(set(e["aliases"]) | {text})
            added += 1
    print(f"derived {added} unambiguous brand aliases", flush=True)


def annotate_flags(entries, countries):
    """Flags are found by country name, not ISO code: title 'Germany', aliases 'DE', 'DEU', 'Germany flag', ..."""
    by_code = {c["cca2"].lower(): c for c in countries}
    demonym_count = collections.Counter(d for c in countries for d in {c.get("demonyms", {}).get("eng", {}).get("m"), c.get("demonyms", {}).get("eng", {}).get("f")} if d)
    for e in entries:
        if e["kind"] != "flag":
            continue
        code, extra = e["name"], set(EXTRA_FLAG_ALIASES.get(e["name"], ()))
        country = by_code.get(code)
        if country:
            title = country["name"]["common"]
            extra |= {country["name"]["official"], country["cca2"], country["cca3"], *country["altSpellings"]}
        else:
            title = FLAG_NAMES.get(code) or " ".join(t.capitalize() for t in code.split("-"))
            extra.add(code)
        extra.add(f"{title} flag")
        if country:
            extra |= {d for d in (country.get("demonyms", {}).get("eng", {}).get("m"), country.get("demonyms", {}).get("eng", {}).get("f")) if d and demonym_count[d] == 1}
        e["title"], e["key"], e["preferred"] = title, title_to_slug(title), True
        e["aliases"] = sorted((set(e["aliases"]) | extra) - {title})
    seen = {}  # two files for one flag (eu / european-union): the shorter name is the default
    for e in sorted((e for e in entries if e["kind"] == "flag"), key=lambda e: len(e["name"])):
        e["preferred"] = seen.setdefault(e["key"], e["id"]) == e["id"]
    return entries


def build_lookup(entries, curated):
    """kind -> normalized string -> id of the preferred entry. Every key, title and alias
    of a brand points at the same file. On a clash: curated aliases (icon_aliases.json,
    a human judgement) beat key, which beats title, which beats alias."""
    lookup, clashes = {"logo": {}}, []
    preferred_logo = {e["key"]: e["id"] for e in entries if e["kind"] == "logo" and e["preferred"]}
    for key, texts in curated.items():
        for text in texts:
            lookup["logo"][title_to_slug(text)] = preferred_logo[key]
    for field in ("key", "title", "alias"):
        for e in entries:
            if not e["preferred"]:
                continue
            for text in e["aliases"] if field == "alias" else [e[field]]:
                n = title_to_slug(text)
                table = lookup.setdefault(e["kind"], {})
                if n and table.setdefault(n, e["id"]) != e["id"]:
                    clashes.append((e["kind"], n, table[n], e["id"]))
    for e in entries:  # explicit variants: 'basetenwordmark', 'anthropicicon'
        suffix = e["name"].split("-")[-1]
        if e["kind"] == "logo" and e["name"] != e["key"] and suffix in VARIANT_TOKENS - THEME_TOKENS:
            lookup["logo"].setdefault(e["key"] + suffix, e["id"])
    for kind, words in QUERY_WORDS.items():  # 'gcp logo', 'gcp icon', 'germany flag', 'flag of germany'
        table = lookup.setdefault(kind, {})
        for n, target in list(table.items()):
            for w in words:
                table.setdefault(n + w, target)
            if kind in QUERY_PREFIXES:
                table.setdefault(QUERY_PREFIXES[kind] + n, target)
    return lookup, clashes


README = """# Icon + logo library

Transparent-background SVGs downloaded from Iconify (https://iconify.design) by
assets-pipeline/build_icon_library.py. `manifest.json` has one entry per file.

| set | kind | license | notes |
|---|---|---|---|
| lucide | icon | ISC | line icons, `currentColor` (recolor by replacing it) |
| logos | logo | CC0 | full-color brand logos |
| thesvg-color | logo | MIT | full-color brand logos, incl. newer brands (e.g. Baseten) |
| simple-icons | logo | CC0 | single-color brand marks, official brand color baked in |
| circle-flags | flag | MIT | flags, titled by country name ("Germany"), not ISO code |
| custom | logo | n/a | hand-supplied SVGs from assets-pipeline/custom_icons (e.g. the Baseten wordmark) |

Lookup: `lookup.json` is `{kind: {normalized string: id}}`. Normalize a label
with `title_to_slug` (lowercase, `+`->plus, `.`->dot, `&`->and, then drop
everything that is not a-z0-9), pick the kind (`logo`, `icon` or `flag`), and
read the id: every key, title and alias of a brand points at the same preferred
file ("GCP", "Google Cloud" and "Google Cloud Platform" all give one icon).
Strip a trailing "logo"/"icon"/"flag" from the query first (flags also have
"<country> flag" aliases). Explicit variants are keyed `<brand>icon` /
`<brand>wordmark` (e.g. `basetenwordmark`). `manifest.json` has the full
entries; `key` groups a brand's variants across sets, `preferred` marks the
default. Curated names live in assets-pipeline/icon_aliases.json.

Brand logos are trademarks of their owners; the SVG licenses above cover the
files, not the marks. Do not imply endorsement.
"""


def main():
    ICONS_DIR.mkdir(parents=True, exist_ok=True)
    brands = brand_data()
    manifest = []
    for prefix, kind, license_id, mono in SETS:
        manifest.extend(build_set(prefix, kind, license_id, mono, brands))
    manifest.extend(build_custom())
    lucide_tags = requests.get(LUCIDE_TAGS_URL, timeout=60).json()
    for e in manifest:
        if e["set"] == "lucide":
            e["tags"] = e["name"].split("-") + lucide_tags.get(e["name"], [])
    curated = json.loads(ALIASES_PATH.read_text())
    annotate(manifest, brands, curated)
    add_derived_aliases(manifest)
    annotate_flags(manifest, requests.get(COUNTRIES_URL, timeout=60).json())
    lookup, clashes = build_lookup(manifest, curated)
    for kind, n, kept, dropped in sorted(set(clashes)):
        print(f"  clash [{kind}] {n!r}: kept {kept}, ignored {dropped}")
    MANIFEST_PATH.write_text(json.dumps(manifest, indent=1) + "\n")
    LOOKUP_PATH.write_text(json.dumps(lookup, indent=None, separators=(",", ":"), sort_keys=True) + "\n")
    (ICONS_DIR / "README.md").write_text(README)
    print(f"total {len(manifest)} icons, {sum(len(t) for t in lookup.values())} lookup strings -> {ICONS_DIR}")


if __name__ == "__main__":
    main()
