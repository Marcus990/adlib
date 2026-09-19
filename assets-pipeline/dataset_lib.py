"""Shared helpers for building the local image + CLIP embedding library.

Used by build_coco_batch.py (initial batch) and add_openimages_batch.py
(later backfill) so both batches produce a consistent, appendable set of
assets/images/*.jpg + assets/embeddings.npy + assets/manifest.json.
"""

import json
import os
from pathlib import Path

import numpy as np
import torch
from PIL import Image
from transformers import CLIPModel, CLIPProcessor

CLIP_CHECKPOINT = "openai/clip-vit-base-patch32"
EMBED_DIM = 512
MAX_SIDE = 1024
JPEG_QUALITY = 80

# LIBRARY_DIR points the scripts at a library outside the repo (e.g. the SD card).
ASSETS_DIR = Path(os.environ.get("LIBRARY_DIR") or Path(__file__).resolve().parent.parent / "assets")
IMAGES_DIR = ASSETS_DIR / "images"
EMBEDDINGS_PATH = ASSETS_DIR / "embeddings.npy"
MANIFEST_PATH = ASSETS_DIR / "manifest.json"


def load_manifest():
    if MANIFEST_PATH.exists():
        with open(MANIFEST_PATH) as f:
            return json.load(f)
    return []


def next_id_start():
    """Return the next integer id to assign, continuing any existing sequence."""
    manifest = load_manifest()
    if not manifest:
        return 1
    return max(int(entry["id"]) for entry in manifest) + 1


def format_id(n):
    return f"{n:06d}"


def resize_image(fileobj_or_path, max_side=MAX_SIDE):
    """Open an image, convert to RGB, and resize so the longest side is
    max_side px (preserving aspect ratio). Returns a PIL.Image."""
    img = Image.open(fileobj_or_path)
    img.load()
    img = img.convert("RGB")
    w, h = img.size
    scale = max_side / max(w, h)
    if scale < 1:
        new_size = (max(1, round(w * scale)), max(1, round(h * scale)))
        img = img.resize(new_size, Image.LANCZOS)
    return img


def save_jpeg(img, dest_path, quality=JPEG_QUALITY):
    img.save(dest_path, "JPEG", quality=quality)


class ClipEmbedder:
    """Wraps CLIPModel/CLIPProcessor for batch image-only embedding."""

    def __init__(self, checkpoint=CLIP_CHECKPOINT, device=None):
        self.device = device or ("cuda" if torch.cuda.is_available() else "cpu")
        self.model = CLIPModel.from_pretrained(checkpoint).to(self.device).eval()
        self.processor = CLIPProcessor.from_pretrained(checkpoint)

    @torch.no_grad()
    def embed_images(self, pil_images):
        """pil_images: list of PIL.Image (RGB). Returns np.ndarray [N, 512] float32."""
        inputs = self.processor(images=pil_images, return_tensors="pt").to(self.device)
        output = self.model.get_image_features(**inputs)
        # transformers>=4.5x wraps the projected embedding in pooler_output;
        # older versions return the tensor directly.
        features = output.pooler_output if hasattr(output, "pooler_output") else output
        features = features.cpu().numpy().astype(np.float32)
        return features


def append_batch(new_manifest_entries, new_embeddings):
    """Append new rows to embeddings.npy and manifest.json, preserving
    index-alignment between the two. Does not overwrite existing rows."""
    assert len(new_manifest_entries) == new_embeddings.shape[0]
    assert new_embeddings.shape[1] == EMBED_DIM

    manifest = load_manifest()
    manifest.extend(new_manifest_entries)

    if EMBEDDINGS_PATH.exists():
        existing = np.load(EMBEDDINGS_PATH)
        combined = np.concatenate([existing, new_embeddings], axis=0)
    else:
        combined = new_embeddings

    assert combined.shape[0] == len(manifest), (
        f"embeddings rows ({combined.shape[0]}) != manifest entries ({len(manifest)})"
    )

    np.save(EMBEDDINGS_PATH, combined)
    with open(MANIFEST_PATH, "w") as f:
        json.dump(manifest, f, indent=2)

    return combined.shape[0]
