#!/usr/bin/env bash
# Prepare a demo image library (e.g. from the SD card) for indexing.
# usage: scripts/prepare_library.sh <source-dir> <dest-dir>
#  - converts jpg/jpeg/png/heic/webp (recursively) to JPEG, longest edge ≤ 1920 px (fast decode, small cache)
#  - id = slugified file name; collisions get a numeric suffix
#  - writes <dest>/captions.tsv (id<TAB>caption) with a caption guessed from the file name — EDIT IT:
#    short concrete descriptions ("red sports car on a mountain road") measurably improve matching.
#  - existing captions.tsv lines are kept (re-runs don't clobber your edits)
# Then: ls-index models/mobileclip-s2 <dest> <dest>/index.json ; ls-calibrate ... <labels.tsv>
set -euo pipefail
SRC="$1"; DEST="$2"
mkdir -p "$DEST"
touch "$DEST/captions.tsv"
n=0
while IFS= read -r -d '' f; do
  base=$(basename "$f"); stem="${base%.*}"
  id=$(echo "$stem" | tr 'A-Z' 'a-z' | sed -E 's/[^a-z0-9]+/-/g; s/^-+|-+$//g')
  [ -z "$id" ] && id="img"
  cand="$id"; i=2
  while [ -e "$DEST/$cand.jpg" ] && [ "$(cat "$DEST/.src-$cand" 2>/dev/null)" != "$f" ]; do cand="$id-$i"; i=$((i+1)); done
  id="$cand"
  sips -Z 1920 -s format jpeg "$f" --out "$DEST/$id.jpg" >/dev/null
  echo "$f" > "$DEST/.src-$id"
  if ! grep -q "^$id	" "$DEST/captions.tsv"; then
    printf '%s\t%s\n' "$id" "$(echo "$stem" | sed -E 's/[-_]+/ /g; s/([a-z])([A-Z])/\1 \2/g' | tr 'A-Z' 'a-z')" >> "$DEST/captions.tsv"
  fi
  n=$((n+1))
done < <(find -L "$SRC" -type f \( -iname '*.jpg' -o -iname '*.jpeg' -o -iname '*.png' -o -iname '*.heic' -o -iname '*.webp' \) -print0)
echo "prepared $n images in $DEST — now edit $DEST/captions.tsv, then index"
