#!/usr/bin/env python3
"""Local stand-in for OpenRouter, for testing the hosted-model code path and its timing without a key.

  POST /api/alpha/decisions        → Jev-shaped {"answers": {"action": {choice, probabilities, confidence}}}
  POST /api/v1/chat/completions    → chat-shaped {"choices":[{"message":{"content": "{\"phrases\":[...]}"}}]}

Behaviour is a simple stand-in for the real models, keyed off library captions (subject = caption before " (").
Latency (ms) is sampled per request: Jev from JEV_MS (default "70,500", skewed toward ~120 like TypeSafe's
"most calls ~100 ms"), chat from CHAT_MS (default "300,550", Flash-Lite ~0.35–0.5 s).
Fault injection: FAIL_RATE (HTTP 500 probability), SLOW_RATE (probability of a 1.5 s stall → client timeout).

usage: mock_openrouter.py <captions.tsv> [port]
"""
import json, random, re, sys, time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import os

caps = [l.split("\t")[1].strip() for l in open(sys.argv[1]) if "\t" in l]
subjects = sorted({c.split(" (")[0] for c in caps}, key=len, reverse=True)
PORT = int(sys.argv[2]) if len(sys.argv) > 2 else 8787
JEV = [int(x) for x in os.environ.get("JEV_MS", "70,500").split(",")]
CHAT = [int(x) for x in os.environ.get("CHAT_MS", "300,550").split(",")]
FAIL = float(os.environ.get("FAIL_RATE", "0"))
SLOW = float(os.environ.get("SLOW_RATE", "0"))


def mentioned(text):
    """Subjects mentioned in text, in order of last occurrence."""
    t = text.lower()
    hits = []
    for s in subjects:
        for m in re.finditer(r"\b" + re.escape(s) + r"s?\b", t):
            hits.append((m.start(), s))
    return [s for _, s in sorted(hits)]


def delay(lo_hi, skew=False):
    lo, hi = lo_hi
    x = random.random() ** (2.5 if skew else 1.0)
    time.sleep((lo + (hi - lo) * x) / 1000)
    if random.random() < SLOW:
        time.sleep(1.5)


class H(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *a):
        pass

    def reply(self, code, obj):
        body = json.dumps(obj).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_POST(self):
        req = json.loads(self.rfile.read(int(self.headers.get("Content-Length", 0))) or b"{}")
        if random.random() < FAIL:
            return self.reply(500, {"error": {"message": "injected failure", "code": 500}})
        if self.path.endswith("/api/alpha/decisions"):
            delay(JEV, skew=True)
            st = req.get("state", {})
            curr, on = st.get("curr", ""), st.get("displayed", {}).get("caption", "")
            new = [s for s in mentioned(curr) if not on.lower().startswith(s)]
            if new and any(w in curr.lower() for w in ("actually", "make that", "instead")) and on != "nothing (blank screen)":
                choice, p = "update", 0.8
            elif new:
                choice, p = "new_render", 0.85
            else:
                choice, p = "no_change", 0.9
            probs = {k: (p if k == choice else (1 - p) / 3) for k in ("no_change", "new_render", "update", "clear")}
            return self.reply(200, {"id": "mock", "model": "jev-mock", "provider": "mock",
                                    "answers": {"action": {"type": "choice", "choice": choice, "confidence": p, "probabilities": probs}},
                                    "usage": {"input_tokens": 300, "output_tokens": 0, "cost": 0}})
        if self.path.endswith("/api/v1/chat/completions"):
            delay(CHAT)
            content = req["messages"][-1]["content"]
            if '"action"' in content and "Reply with JSON only" in content:
                out = {"action": "no_change"}
            else:
                newest = content.split("Newest speech:")[-1].split("\nLibrary:")[0]
                subs = mentioned(newest)
                out = {"phrases": list(dict.fromkeys(reversed(subs)))[:3] or [" ".join(newest.split()[-3:])]}
            return self.reply(200, {"id": "mock", "choices": [{"message": {"role": "assistant", "content": json.dumps(out)}}]})
        self.reply(404, {"error": "not found"})


print(f"mock OpenRouter on :{PORT}  subjects={len(subjects)} JEV_MS={JEV} CHAT_MS={CHAT} FAIL={FAIL} SLOW={SLOW}", flush=True)
class Server(ThreadingHTTPServer):
    daemon_threads = True
    request_queue_size = 64

    def server_bind(self):
        # HTTPServer.server_bind does a reverse-DNS getfqdn() before listening, which hangs when the
        # network is asleep (socket stuck un-listened). Skip it.
        import socketserver
        socketserver.TCPServer.server_bind(self)
        self.server_name, self.server_port = "localhost", self.server_address[1]


Server(("127.0.0.1", PORT), H).serve_forever()
