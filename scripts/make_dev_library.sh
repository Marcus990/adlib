#!/usr/bin/env bash
# Builds a dev image library from macOS's built-in account pictures (not committed: Apple assets).
# Output: dev-library/<category>-<name>.jpg + dev-library/captions.tsv (id<TAB>caption)
set -euo pipefail
OUT="${1:-dev-library}"
mkdir -p "$OUT"
: > "$OUT/captions.tsv"
find "/Library/User Pictures" -name '*.heic' | sort | while read -r f; do
  cat=$(basename "$(dirname "$f")" | tr 'A-Z' 'a-z')
  name=$(basename "$f" .heic)
  id="${cat}-$(echo "$name" | tr 'A-Z ' 'a-z-')"
  sips -s format jpeg "$f" --out "$OUT/$id.jpg" >/dev/null
  printf '%s\t%s\n' "$id" "$(echo "$name" | tr 'A-Z' 'a-z') ($cat)" >> "$OUT/captions.tsv"
done
echo "built $(wc -l < "$OUT/captions.tsv") images in $OUT"
