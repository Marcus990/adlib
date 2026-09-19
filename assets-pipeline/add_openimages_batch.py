"""Step 6: backfill Open Images V7 into the same library, appending (not
replacing) whatever is already in assets/images + embeddings.npy + manifest.json.

Run this only after the COCO batch (build_coco_batch.py) has been proven
working end-to-end - it continues the id sequence from wherever COCO (or any
prior batch) left off.

Usage:
    python assets-pipeline/add_openimages_batch.py \
        --classes classes.txt \
        --samples-per-class 25 \
        --batch-size 32

classes.txt: one Open Images class name per line (e.g. "Cat", "Bicycle").
Aim for ~300-500 classes at ~20-30 samples/class per the plan.

Requires the `fiftyone` package (already in requirements) - it downloads only
the requested subset of Open Images V7 via FiftyOne's zoo dataset loader, not
the full dataset.
"""

import argparse
import sys
from pathlib import Path

import fiftyone as fo
import fiftyone.zoo as foz
import numpy as np
from tqdm import tqdm

sys.path.insert(0, str(Path(__file__).resolve().parent))
import dataset_lib as dl


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--classes", required=True, help="Text file, one Open Images class per line")
    parser.add_argument("--samples-per-class", type=int, default=25)
    parser.add_argument("--batch-size", type=int, default=32)
    parser.add_argument("--split", default="train", choices=["train", "validation", "test"])
    args = parser.parse_args()

    classes = [line.strip() for line in open(args.classes) if line.strip()]
    print(f"Loading Open Images V7 subset: {len(classes)} classes, "
          f"~{args.samples_per_class} samples/class, split={args.split}")

    dataset = foz.load_zoo_dataset(
        "open-images-v7",
        split=args.split,
        label_types=["detections"],
        classes=classes,
        max_samples=len(classes) * args.samples_per_class,
        shuffle=True,
        seed=51,
    )

    dl.IMAGES_DIR.mkdir(parents=True, exist_ok=True)
    start_id = dl.next_id_start()
    print(f"Starting at id {dl.format_id(start_id)} (continuing existing sequence).")

    embedder = dl.ClipEmbedder()
    print(f"CLIP checkpoint: {dl.CLIP_CHECKPOINT} on device {embedder.device}")

    manifest_entries = []
    all_embeddings = []
    batch_imgs, batch_entries = [], []

    def flush_batch():
        if not batch_imgs:
            return
        embs = embedder.embed_images(batch_imgs)
        all_embeddings.append(embs)
        manifest_entries.extend(batch_entries)
        batch_imgs.clear()
        batch_entries.clear()

    for i, sample in enumerate(tqdm(dataset, desc="processing")):
        new_id = start_id + i
        id_str = dl.format_id(new_id)
        filename = f"{id_str}.jpg"
        dest_path = dl.IMAGES_DIR / filename

        img = dl.resize_image(sample.filepath)
        dl.save_jpeg(img, dest_path)

        # primary class label = first detection's label, if any (QA/debug only)
        class_label = None
        detections = getattr(sample, "ground_truth", None)
        if detections and detections.detections:
            class_label = detections.detections[0].label

        entry = {
            "id": id_str,
            "filename": filename,
            "source_dataset": "open_images_v7",
        }
        if class_label:
            entry["class_label"] = class_label

        batch_imgs.append(img)
        batch_entries.append(entry)

        if len(batch_imgs) >= args.batch_size:
            flush_batch()

    flush_batch()

    if not manifest_entries:
        print("No new images processed.")
        return

    combined_embeddings = np.concatenate(all_embeddings, axis=0)
    total = dl.append_batch(manifest_entries, combined_embeddings)
    print(f"Appended {len(manifest_entries)} images. Total rows in library: {total}")


if __name__ == "__main__":
    main()
