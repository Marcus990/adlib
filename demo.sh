#!/usr/bin/env bash
# Demo launcher. Examples:
#   ./demo.sh                      # AirPods mic, full screen on this display
#   ./demo.sh airpods 1            # AirPods, stage full screen on display 1 (projector), debug stays here
#   ./demo.sh builtin              # MacBook microphone
#   ./demo.sh replay [talk.wav]    # rehearsal replay (default: fixtures/audio/rehearsal-3min.wav)
#   ./demo.sh window …             # any of the above, but windowed instead of full screen
# Library: INDEX=<lib>/index.json ./demo.sh …  (default: dev-library/index.json)
set -euo pipefail
cd "$(dirname "$0")"
FULL=1
if [ "${1:-}" = "window" ]; then FULL=; shift; fi
MODE="${1:-airpods}"; ARG="${2:-}"
case "$MODE" in
  airpods) export LS_SOURCE="mic:AirPods" ;;
  builtin) export LS_SOURCE="mic:MacBook Air Microphone" ;;
  replay)  export LS_SOURCE="wav:$PWD/${ARG:-fixtures/audio/rehearsal-3min.wav}"; ARG="" ;;
  mic:*|wav:*) export LS_SOURCE="$MODE" ;;
  *) echo "usage: ./demo.sh [window] airpods|builtin|replay [display|wav]"; exit 2 ;;
esac
[ -n "$ARG" ] && export LS_DISPLAY="$ARG"
[ -n "$FULL" ] && export LS_FULLSCREEN=1
[ -x target/release/live-slides ] || { echo "build first: CARGO_BUILD_JOBS=2 cargo build --release"; exit 1; }
grep -qE '^OPENROUTER_API_KEY=.+' .env 2>/dev/null || echo "note: no OPENROUTER_API_KEY in .env — using local fallbacks"
echo "source=$LS_SOURCE display=${LS_DISPLAY:-this} fullscreen=${FULL:+yes} index=${INDEX:-dev-library/index.json}"
echo "stage keys: f full screen · Esc exit full screen · b blank · g grid"
exec ./target/release/live-slides
