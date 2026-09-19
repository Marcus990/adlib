#!/usr/bin/env bash
# One-command check of the three things that could not be verified unattended.
# Run from the repo root, at the Mac, with the AirPods connected:   ./scripts/morning_check.sh [mic-name]
set -uo pipefail
MIC="${1:-AirPods}"
cd "$(dirname "$0")/.."
ok() { printf '  \033[32m✓\033[0m %s\n' "$*"; }
bad() { printf '  \033[31m✗\033[0m %s\n' "$*"; }

echo "1) OpenRouter key"
if grep -qE '^OPENROUTER_API_KEY=.+' .env 2>/dev/null; then ok ".env has OPENROUTER_API_KEY"; else bad "put OPENROUTER_API_KEY=... in .env (cp .env.example .env)"; fi

echo "2) Microphone ($MIC) — approve the macOS microphone prompt if one appears, then speak for 3 s"
if out=$(timeout 20 ./target/release/ls-hear --probe "$MIC" 3 2>&1 | grep -E 'device=|Error'); then
  echo "     $out"
  case "$out" in *peak=0.0000*) bad "captured only silence — wrong device or muted?";; *device=*) ok "audio arrives";; *) bad "capture failed";; esac
else bad "probe failed/timed out (see: ./target/release/ls-hear --list)"; fi

echo "3) Hosted models on the 2-minute rehearsal (≈ 2.5 min)"
./scripts/guard.sh 2800 ./target/release/ls-replay fixtures/audio/rehearsal-3min.wav >/dev/null 2>/tmp/ls-morning.err
L=$(ls -t logs/run-*.jsonl | head -1)
python3 scripts/eval_run.py "$L" fixtures/audio/rehearsal-3min.expected.tsv | tail -4
python3 - "$L" <<'EOF'
import json, sys, statistics as st, collections
ev = [json.loads(l) for l in open(sys.argv[1])]
src = collections.Counter(e["source"] for e in ev if e.get("ev") == "decide")
jev = [e["ms"] for e in ev if e.get("ev") == "decide" and e["source"] == "Jev"]
q = [e["query_ms"] for e in ev if e.get("ev") == "search" and not e["fallback"]]
print("   decide sources:", dict(src))
if jev: print(f"   Jev ms p50={st.median(jev):.0f} max={max(jev)}")
if q: print(f"   query-model ms p50={st.median(q):.0f} max={max(q)}")
if not jev: print("   ✗ no Jev answers — check /tmp/ls-morning.err")
EOF
echo
echo "Next: live run → LS_SOURCE=mic:$MIC LS_FULLSCREEN=1 ./target/release/live-slides"
