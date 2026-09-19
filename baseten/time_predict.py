"""Time SDXL Lightning predict calls at several resolutions.

Usage: python baseten/time_predict.py [--url URL] [--sizes 1024 768 512] [--prompt TEXT] [--save DIR]
Reads BASETEN_API_KEY from baseten/.env or the environment.
"""

import argparse
import base64
import io
import os
import time
from pathlib import Path

import requests
from PIL import Image

HERE = Path(__file__).resolve().parent
DEFAULT_URL = "https://model-3yvmgyn3.api.baseten.co/deployment/qrmdkv1/predict"


def load_key():
    key = os.environ.get("BASETEN_API_KEY")
    if key:
        return key
    for line in (HERE / ".env").read_text().splitlines():
        if line.startswith("BASETEN_API_KEY="):
            return line.split("=", 1)[1].strip()
    raise SystemExit("BASETEN_API_KEY not found")


def call(url, key, prompt, size):
    body = {"prompt": prompt}
    if size:
        body.update(width=size, height=size)
    t0 = time.monotonic()
    r = requests.post(url, headers={"Authorization": f"Api-Key {key}"}, json=body, timeout=300)
    dt = time.monotonic() - t0
    r.raise_for_status()
    data = r.json()
    img = Image.open(io.BytesIO(base64.b64decode(data["result"])))
    return dt, len(r.content), img, data.get("generation_time_s")


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--url", default=DEFAULT_URL)
    p.add_argument("--sizes", type=int, nargs="*", default=[512])
    p.add_argument("--prompt", default="a squirrel eating a piece of bamboo")
    p.add_argument("--save", default=None)
    a = p.parse_args()
    key = load_key()
    for size in a.sizes:
        dt, nbytes, img, gen = call(a.url, key, a.prompt, size)
        gen_s = f"{gen:.2f}s" if gen is not None else "n/a (deployment doesn't report it)"
        print(f"size={size} -> {img.size} round_trip={dt:.2f}s generation={gen_s} response={nbytes/1024:.0f}KB", flush=True)
        if a.save:
            Path(a.save).mkdir(parents=True, exist_ok=True)
            img.save(Path(a.save) / f"out_{size}.jpg")


if __name__ == "__main__":
    main()
