"""Step 2-5: ingest COCO val2017 into assets/images + embeddings.npy + manifest.json.

Usage:
    python assets-pipeline/build_coco_batch.py --zip /path/to/val2017.zip [--limit N] [--batch-size 32]

Downloads nothing itself (the zip is large and slow to fetch repeatedly) -
point it at an already-downloaded val2017.zip. Reads images directly out of
the zip (no bulk extraction to disk), resizes/re-encodes each one, assigns it
the next sequential id, embeds it with CLIP, and appends to the shared
manifest/embeddings files via dataset_lib.append_batch.
"""

import argparse
import sys
import zipfile

from tqdm import tqdm

sys.path.insert(0, str(__import__("pathlib").Path(__file__).resolve().parent))
import dataset_lib as dl


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--zip", required=True, help="Path to val2017.zip")
    parser.add_argument("--limit", type=int, default=None, help="Only process first N images (debugging)")
    parser.add_argument("--batch-size", type=int, default=32)
    args = parser.parse_args()

    dl.IMAGES_DIR.mkdir(parents=True, exist_ok=True)

    with zipfile.ZipFile(args.zip) as zf:
        names = sorted(
            n for n in zf.namelist()
            if n.lower().endswith(".jpg") and not n.endswith("/")
        )
        if args.limit:
            names = names[: args.limit]

        start_id = dl.next_id_start()
        print(f"Found {len(names)} images in zip. Starting at id {dl.format_id(start_id)}.")

        embedder = dl.ClipEmbedder()
        print(f"CLIP checkpoint: {dl.CLIP_CHECKPOINT} on device {embedder.device}")

        manifest_entries = []
        all_embeddings = []

        batch_imgs = []
        batch_entries = []

        def flush_batch():
            if not batch_imgs:
                return
            embs = embedder.embed_images(batch_imgs)
            all_embeddings.append(embs)
            manifest_entries.extend(batch_entries)
            batch_imgs.clear()
            batch_entries.clear()

        for i, name in enumerate(tqdm(names, desc="processing")):
            new_id = start_id + i
            id_str = dl.format_id(new_id)
            filename = f"{id_str}.jpg"
            dest_path = dl.IMAGES_DIR / filename

            with zf.open(name) as f:
                img = dl.resize_image(f)
            dl.save_jpeg(img, dest_path)

            batch_imgs.append(img)
            batch_entries.append({
                "id": id_str,
                "filename": filename,
                "source_dataset": "coco_val2017",
            })

            if len(batch_imgs) >= args.batch_size:
                flush_batch()

        flush_batch()

    if not manifest_entries:
        print("No new images processed.")
        return

    import numpy as np
    combined_embeddings = np.concatenate(all_embeddings, axis=0)
    total = dl.append_batch(manifest_entries, combined_embeddings)
    print(f"Appended {len(manifest_entries)} images. Total rows in library: {total}")
    print(f"Images:     {dl.IMAGES_DIR}")
    print(f"Embeddings: {dl.EMBEDDINGS_PATH}")
    print(f"Manifest:   {dl.MANIFEST_PATH}")


if __name__ == "__main__":
    main()
