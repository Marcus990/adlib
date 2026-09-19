# Assets handoff — photo library, icons/logos, aliases, recoloring, image-gen fallback

Written 2026-09-19 for the Claude Code session (and human) merging this into the Rust backend.
Everything here was built in Python, outside the Rust workspace. **Nothing in this commit touches your
crates, app, or existing scripts** — it only adds `assets-pipeline/`, `baseten/`, and this file. The Rust
integration is the to-do list below and has not been started.

Based on `origin/main` @ `700295e`. If your local main is ahead of that, `git pull` will merge cleanly (no
file overlaps; your `.gitignore` is untouched).

## Decision already made (Marcus, 2026-09-19)

**Photo search uses OUR method:** OpenAI CLIP ViT-B/32 image embeddings precomputed on the SD card
(`embeddings.npy`), queried with the same model's text encoder. Do **not** re-index the photos with
`ls-index`/MobileCLIP-S2. The two are different embedding spaces (both happen to be 512-d) and cannot be mixed.
Icons/logos/flags use text lookup (no embeddings) — see §3.

## 1. What exists and where (the SD card is NOT in git)

`/Volumes/NO NAME/assets/` (FAT32, 15 GB card, ~2.2 GB used). Rebuild scripts are in `assets-pipeline/`.

| path | what |
|---|---|
| `images/000001.jpg …` | 15,000 JPEGs (q80, **longest side ≤ 1024 px**): 10,000 Open Images V7 + 5,000 COCO val2017 |
| `manifest.json` | 15,000 entries `{id, filename, source_dataset, class_label?}`; row `i` ↔ `embeddings.npy[i]` |
| `embeddings.npy` | float32 `(15000, 512)`, OpenAI `clip-vit-base-patch32`, **image tower only**, 128-byte npy header |
| `icons/` | 13,438 transparent SVGs + `manifest.json` + `lookup.json` + `README.md` (§3) |

Facts that matter (measured this session):
- **Embeddings are NOT L2-normalized** (row norms ≈ 10–11.6). Normalize rows at load (or divide in the cosine).
- **Only the 10,000 Open Images entries have `class_label`** — single generic words ("Dog", "Tree", "Woman").
  The 5,000 COCO entries have no label or caption at all; the embedding is their only searchable signal.
- Resolution: none of 300 sampled images reached 1920 px (median long side 1024). Your stage is full-bleed, so
  these upscale ~1.9× on a 1080p display. Your own TODO C4 asks for ≥1920 px.
- Licensing is unresolved: Open Images photos are (to our understanding) CC-BY 2.0 and COCO's Flickr photos carry
  mixed licenses incl. non-commercial. **Verify before shipping commercially.** The photo manifest stores no
  author/license, so attribution is currently impossible.

Rebuild (`pip install -r assets-pipeline/requirements.txt`; `LIBRARY_DIR` points at the card):
```
LIBRARY_DIR="/Volumes/NO NAME/assets" python assets-pipeline/build_coco_batch.py --zip val2017.zip   # see docstring
LIBRARY_DIR="/Volumes/NO NAME/assets" python assets-pipeline/add_openimages_batch.py ...            # see docstring
LIBRARY_DIR="/Volumes/NO NAME/assets" python assets-pipeline/build_icon_library.py                  # icons, ~2 min, needs network
LIBRARY_DIR="/Volumes/NO NAME/assets" python assets-pipeline/icon_search.py logo "gcp logo"         # reference search
```
The icon build is idempotent (skips existing SVGs, rebuilds manifest/lookup) but pulls the *current* upstream
Iconify data, so a re-run can differ slightly from the shipped card.

## 2. Photo search — TO DO in Rust (P0)

Goal: text query → best photo from our 15,000, using our embeddings.

- [x] **AS1 DONE (Rust side, 09-19).** `crates/search/src/assets.rs::ClipText` — text tower only, built from
      `openai/clip-vit-base-patch32`'s `pytorch_model.bin` (that repo has no safetensors) and converted once into
      `models/clip-vit-b32/clip-text-vit-b32.safetensors` (254 MB) so later starts mmap it. **Measured on the
      8 GB M2: 19–21 ms per phrase on CPU** (MobileCLIP was 21 ms, so no latency regression), model load 1.3 s
      warm / 3.0 s on the converting run. Original task text below.
      **AS1 (me, M, P0) CLIP ViT-B/32 text encoder in Candle.** Load `openai/clip-vit-base-patch32`
      (HF; `model.safetensors` + `tokenizer.json`, context 77). Only the *text* tower runs at query time; images
      are precomputed. Check `candle_transformers::models::clip` supports the ViT-B/32 config
      (`ClipConfig::vit_base_patch32()`); output = projected text features, then L2-normalize. **Measure CPU
      latency of one query on the 8 GB Mac** (not measured by us; your MobileCLIP-S2 choice was made for speed).
- [x] **AS2 DONE (09-19).** `assets::load_index` parses npy v1/v2 (little-endian f32, C order, shape checked
      against the manifest), L2-normalizes rows on load, and builds the existing `Index`/`Entry` in memory, so
      `Match`, the LRU cache and `img://` are untouched. `file` is relative (`images/<filename>`), root =
      `LS_ASSETS`. COCO rows keep an empty caption and read as "photo" in board summaries. Prompts and the
      named-subject shortcut now use `Index::vocab(200)` (distinct labels by frequency) instead of 15k captions;
      image prefetch is skipped above 256 entries. Unit-tested with a synthetic card. Original task text below.
      **AS2 (me, S, P0) `.npy` + manifest loader.** Parse npy v1 (little-endian f32, shape `(N,512)`, 128-byte
      header — file size is exactly `15000*512*4 + 128`), L2-normalize rows, keep in RAM (~31 MB). Build your
      existing in-memory `Entry {id, file, caption, img, cap}` from it (`caption` = `class_label` or empty,
      `cap` empty) so `Match`, the LRU image cache and the `img://` protocol keep working unchanged. Point at the
      library with a new env var (suggest `LS_ASSETS=/Volumes/NO NAME/assets`); resolve `file` relative to it
      (your C1: no absolute paths).
- [~] **AS3 IN PROGRESS.** `TAU` defaults to **0.25** whenever `LS_ASSETS` is set (0.52 stays for MobileCLIP),
      but this is a guess from your handoff, **not calibrated** — the card was not mounted on this machine, so no
      real scores exist yet. `ls-assets <assets-dir> <clip-text-dir> "phrase"…` prints the top 5 with scores and
      timings; run it on the card to pick the threshold. Original task text below.
      **AS3 (me, S, P0) Recalibrate `TAU`.** 0.52 was calibrated for MobileCLIP. CLIP ViT-B/32 image–text cosines
      are typically much lower (expect roughly 0.2–0.35 for good matches — verify), so the current default would
      reject nearly everything. Use `ls-calibrate` with a phrase→image set (your C5).
- [ ] **AS4 (me, S, P1) Query phrasing.** CLIP text tends to match better with "a photo of a {x}". A/B it against
      bare noun phrases on the calibration set; your query model's phrases feed this.
- [ ] **AS5 (both, P1) Retrieval-quality check.** We have NOT tested text→image retrieval quality against
      `embeddings.npy` in this session (embeddings were built earlier). Do it before trusting the numbers.
- [ ] **AS6 (you, P1) Decide the curated demo subset.** 15k un-captioned ≤1024 px snapshots are a recall tool, not
      a polished deck library. Consider a small curated ≥1920 px CC0 tier on top (see §6).

## 3. Icons, logos, flags — what was built

`icons/manifest.json` (13,438 entries; kinds `logo` 10,795 · `icon` 1,925 · `flag` 718). Sets and licenses:
`lucide` (ISC, generic UI icons, 1,925) · `logos` (CC0, full-color brands, 2,174) · `thesvg-color` (MIT, full-color
brands incl. newer ones e.g. Baseten, 4,887) · `simple-icons` (CC0, mono brand marks with the official brand color
baked in, 3,733) · `circle-flags` (MIT, 718) · `custom` (1: `custom:baseten-wordmark`, hand-cleaned lockup,
transparent, `currentColor`). Brand logos are trademarks of their owners; the file licenses don't cover the marks.

Entry fields: `id` (`set:name`), `set`, `name`, `kind`, `monochrome` (draws in `currentColor`; recolor by string
replace), `color` (baked hex, simple-icons only), `aliases`, `tags` (lucide: name tokens + Lucide's own tag
phrases), `license`, `file` (relative to `icons/`), `aspect` (w/h), `key` (brand group), `title` (display name),
`preferred` (the default file for its `key`; 8,326 entries). SVGs carry a `viewBox` and a 512-px `width/height`.

`icons/lookup.json` = `{kind: {normalized_string: id}}`, 30,926 strings (1.1 MB). Every key, title and alias of a
brand points at the **same preferred file** ("GCP", "Google Cloud", "Google Cloud Platform" → `logos:google-cloud`).
Also contains "X logo", "X icon", "X flag", "flag of X" forms and explicit variants (`basetenwordmark`,
`anthropicicon`). **Normalization (port exactly):**
```python
REPL = {"+": "plus", ".": "dot", "&": "and", "đ": "d", "ħ": "h", "ı": "i", "ĸ": "k", "ŀ": "l", "ł": "l", "ß": "ss", "ŧ": "t"}
def title_to_slug(s):  # lowercase, per-char REPL, Unicode NFD, drop everything not a-z0-9
    s = "".join(REPL.get(c, c) for c in s.lower())
    return re.sub(r"[^a-z0-9]", "", unicodedata.normalize("NFD", s))
```
Precedence on a clash: curated (`assets-pipeline/icon_aliases.json`) > key > title > alias. Decisions baked in:
"twitter" → old bird, "x" → new X logo (separate brands); "Lambda" → AWS Lambda (the other Lambda logo is only
reachable by id `thesvg-color:lambda`); "Apache" → `logos:apache-http`; AWS/GCP/Azure/Apache name variants across
sets are merged (57 merges). Default preference among a brand's files: not `-light/-dark` → the mark (`-icon`) →
full color over mono (`logos` > `thesvg-color` > `simple-icons`).

Search beyond exact match is specified by the reference implementation **`assets-pipeline/icon_search.py`** (port it):
1. drop filler words (logo, icon, flag, the, of, …); try every contiguous word span against `lookup.json`,
   longest first (confidence 0.7 + 0.3·coverage);
2. if nothing ≥ 0.85: fuzzy — char-trigram Dice vs every lookup string, a prefix bonus (`postgre`), and
   optimal-string-alignment edit distance ≤ 2 for `len ≥ 4` (`japn`, `kubernets`); confidence ×0.95;
3. for `kind=icon`: token overlap against each icon's name + Lucide tags (concept search: "security lock");
4. `pick()` auto-selects when top ≥ **0.85**, or ≥ **0.6** and ≥ **0.15** ahead of #2; otherwise `None` + the
   candidates (hand the top ~10 to the model). Nonsense ("zzqxv") returns nothing. Tested: 27 queries in Python
   (typos, phrases, flags, concepts) — the two initial misses were fixed; ~1–29 ms/query in pure Python.
`kind` is a required input (`logo` | `icon` | `flag`). Suggested `search_any`: try `logo`, fall back to `icon`.
"database" → `icon` = `lucide:database`; as `logo` it returns nothing confident (correct).

Known gaps: only ~6% of brands have an alias beyond their own name (fine for most; short forms like "GCP", "K8s",
"S3" are curated, ~90 entries + 163 auto-derived). Lucide's tags lack some synonyms ("growth" isn't on
`trending-up`), so "growth chart" won't auto-pick — pass the model the candidates. Flags are found by country
name ("Germany", "German", "USA", "UK"); ~450 regional/language flags have only derived titles ("Au Nsw").

## 4. Wiring icons into diagrams — TO DO in Rust

Today (`crates/canvas`, `crates/agent`): `Node.icon: Option<String>` is an **emoji** — the tool schema says
`"icon": "optional single emoji"`, the agent prompt asks for one per node, `clean_nodes` clamps it to 4 chars.
Marcus does not want emoji/AI-rendered icons; use the library instead.

- [ ] **AS7 (me, M, P0) Resolve icons in Rust, not in the LLM.** On `draw_diagram`/`extend_diagram`, look up each
      node `label` in `lookup.json` (`logo`, then `icon`; **precision over recall** — only attach on an exact
      lookup hit or a confident `pick()`, else render the node with no icon; a wrong logo is worse than none).
      Store the asset `id` in `Node.icon` (change the type; stop asking the agent for emoji; remove the emoji
      clamp). The LLM never sees or emits asset ids.
- [ ] **AS8 (me, M, P0) Serve + draw SVGs.** Add an `icon://` protocol beside `img://` (bytes from
      `$LS_ASSETS/icons/<file>`, small LRU) and draw them inside diagram nodes in the web view. Manifest + lookup
      load once at startup (~5.8 MB JSON total).
- [ ] **AS9 (me, S, P1) Direct requests** ("show me the Amazon logo"): new tile kind (`logo`) next to
      `image|diagram|chart`, or a one-node diagram — pick one; single-node is the cheaper first cut.
- [ ] **AS10 (both, P1) Theme decision.** The `sketch` theme is hand-drawn paper; full-color brand logos will
      clash. Options: a taped "sticker" card behind the logo, or a pencil/grayscale filter for icons. The `slate`
      theme is dark, so recoloring (§5) is required there.

## 5. Recoloring — spec, NOT implemented (P1)

No ML needed; deterministic transform in Rust on the SVG string.
- **Mono icons** (`monochrome: true`; all Lucide + the custom Baseten wordmark): replace `currentColor` with the
  theme foreground. Black marks (GitHub, Baseten wordmark) vanish on dark slides otherwise.
- **Full-color brand logos: never recolor** (brand integrity). If contrast against the slide background is too
  low, prefer a themed sibling: 434 `-light`/`-dark` variants exist (mostly `thesvg-color`, 14 in `logos`); they share the brand `key`
  but are not `preferred` (find them by `key` + name suffix — selection logic is not implemented).
- **Simple Icons** carry a baked brand hex in `color` (some are near-black); 279 brands have no official color and
  fall back to black.
- **Contrast check:** compute contrast of large shapes vs the slide background; if < ~3:1, swap for the light/dark
  sibling, invert lightness for mono shapes, or put the icon on a surface-colored card (always works).
- Future illustrations (§6): one accent color (unDraw-style) is a string substitution; multi-color sets need role
  mapping (accent / neutral / skin — leave skin alone).

## 6. Image-generation fallback — TO DO (P0/P1); NOT in the repo

Your repo has **no** generation code (grep confirms). What exists:
- `baseten/sdxl-lightning/` — a Truss model: SDXL base + ByteDance SDXL-Lightning 4-step UNet, fp16, L4 GPU.
  `predict({"prompt", "width"=512, "height"=512})` → `{"result": <base64 JPEG>, "generation_time_s"}`; dims must be
  multiples of 8. `config.yaml` has no secrets. Deploy with `truss push` (needs a Baseten account).
- `baseten/time_predict.py` — timing harness (`--sizes 512 768 1024 --save DIR`). It contains our deployed
  endpoint URL (`DEFAULT_URL`); calling it needs `BASETEN_API_KEY` in `baseten/.env` or the environment.
  **We did not record latency numbers in the repo — re-measure** (including cold start vs warm) before designing
  around them.

- [ ] **AS11 (me, M, P0) `crates/gen`:** `POST` to the predict URL with header `Authorization: Api-Key $BASETEN_API_KEY`,
      body `{prompt,width,height}`, decode base64 → bytes. Own timeout; never blocks the pipeline (like
      `crates/query`). Add `BASETEN_API_KEY=` and the URL to `.env.example`. **Never commit `.env`** (already
      ignored by your `.gitignore`).
- [ ] **AS12 (both, P0) Trigger policy.** Generate only when display-intent says show AND the best library score
      is below `TAU` (after AS3). Generation will exceed your ~1 s join window: reuse the stage "pending" mechanism
      — hold the previous image / a placeholder, then swap in when it lands; drop it if the talk moved on.
- [ ] **AS13 (me, S, P1) Persist + gap report.** Save generated images into a `generated/` folder with their
      prompt as caption and log the miss — this is your C3 gap report (`scripts/gaps.py`). Photos only.
- [ ] **AS14 (both, P1) Policy:** never generate logos, icons or text-bearing images (models render them badly —
      the reason the icon library exists); those come from the library or are omitted. Match the prompt style to
      the active theme. 512 px is the fast default; the stage is full-bleed, so decide upscale vs `1024`.

## 7. Suggested order

1. AS1–AS3 (photo search on our embeddings, calibrated) → measure. 2. AS7–AS8 (icons in diagram nodes).
3. AS11–AS12 (generation fallback, after AS3 so "no good match" is meaningful). 4. AS9, AS10, then §5 recoloring.
5. AS4–AS6, AS13–AS14, licensing + asset roadmap.

## 8. Asset roadmap (not started; ordered by value)

Photo licensing/resolution audit (per-photo author + license, a CC0 ≥2560 px tier) · recolorable vector
illustrations for concepts ("teamwork", "growth") — check licenses (unDraw restricts bundling; Open Doodles is
CC0) · fonts (open license + mono + emoji fallback), palettes, slide layouts, procedurally generated backgrounds ·
maps/geo (Natural Earth, public domain) · official AWS/Azure/GCP architecture icon sets, device/browser frames ·
avatars, animated assets. Log every "no confident match" query — it tells you what to acquire next.

## 9. Verified vs not

Verified here: Python search tests (27 cases) and SVG rendering with `resvg` (transparent on a checkerboard;
Anthropic/OpenAI/Google/Baseten/GitHub/Slack/flags/Lucide look right); all lookup ids resolve to existing files;
every brand group has exactly one `preferred`; the Baseten wordmark differs from the source only in anti-aliased
edge pixels. **Not verified:** anything in Rust; end-to-end behavior; text→image retrieval quality on
`embeddings.npy`; generation latency; the recolor logic (unwritten).

Secrets: nothing secret is committed. `baseten/.env` (the Baseten key) is local only and ignored; put the key in
your own `.env`.


## Status from the Rust side (2026-09-19, Claude Code session)

Done: **AS1, AS2** (photo search runs on your embeddings behind `LS_ASSETS`), plus the `ls-assets` query CLI.
Not done: AS3 calibration (needs the card), AS4–AS6, AS7–AS10 (icons), AS11–AS14 (generation).

Blocked on hardware: the card was never mounted here (`/Volumes` had only Macintosh HD), so **retrieval quality
and TAU are unverified** — AS5 remains open. Everything else was measured: text encode 19–21 ms/phrase on CPU,
index load 0.6 ms per 64 rows (≈150 ms for 15k, plus reading 31 MB off the card), brute-force search 48 µs per
64 rows (≈11 ms over 15k).

To try it: `LS_ASSETS="/Volumes/NO NAME/assets" ./demo.sh window airpods`.
