#!/usr/bin/env bash
# Usage: scripts/guard.sh <max_rss_mb> <cmd...>
# Runs cmd; kills it if its resident memory exceeds max_rss_mb. Protects the 8 GB dev machine.
set -u
max=$1; shift
"$@" &
pid=$!
while kill -0 "$pid" 2>/dev/null; do
  rss=$(ps -o rss= -p "$pid" 2>/dev/null | tr -d ' ')
  if [ -n "$rss" ] && [ "$rss" -gt $((max * 1024)) ]; then
    echo "guard: RSS ${rss}KB > ${max}MB, killing $pid" >&2
    kill -9 "$pid"; exit 137
  fi
  sleep 0.5
done
wait "$pid"
