"""One-off repair: the first Open Images backfill run used the wrong field
name (`sample.detections` instead of `sample.ground_truth`), so none of its
10,000 manifest entries got a class_label. This reprocesses the same cached
raw images through the identical resize/encode pipeline and matches them to
existing assets/images/*.jpg by content hash (not by re-derived iteration
order, which isn't guaranteed stable across process runs) to safely backfill
class_label without touching images or embeddings.
"""

import hashlib
import io
import json
import sys
from pathlib import Path

import fiftyone.zoo as foz

sys.path.insert(0, str(Path(__file__).resolve().parent))
import dataset_lib as dl


def md5_of_file(path):
    return hashlib.md5(Path(path).read_bytes()).hexdigest()


def main():
    classes = [l.strip() for l in open("assets-pipeline/openimages_classes.txt") if l.strip()]
    dataset = foz.load_zoo_dataset(
        "open-images-v7",
        split="train",
        label_types=["detections"],
        classes=classes,
        max_samples=len(classes) * 25,
        shuffle=True,
        seed=51,
    )
    print(f"Reloaded dataset with {len(dataset)} samples (should be cached, no download)")

    hash_to_label = {}
    for sample in dataset.iter_samples(progress=True):
        img = dl.resize_image(sample.filepath)
        buf = io.BytesIO()
        dl.save_jpeg(img, buf)
        digest = hashlib.md5(buf.getvalue()).hexdigest()

        label = None
        gt = getattr(sample, "ground_truth", None)
        if gt and gt.detections:
            label = gt.detections[0].label
        if label:
            hash_to_label[digest] = label

    print(f"Built hash->label map for {len(hash_to_label)} samples")

    manifest = dl.load_manifest()
    matched = 0
    for entry in manifest:
        if entry["source_dataset"] != "open_images_v7":
            continue
        img_path = dl.IMAGES_DIR / entry["filename"]
        digest = md5_of_file(img_path)
        label = hash_to_label.get(digest)
        if label:
            entry["class_label"] = label
            matched += 1

    print(f"Matched {matched} / 10000 existing open_images_v7 entries to a class_label")

    with open(dl.MANIFEST_PATH, "w") as f:
        json.dump(manifest, f, indent=2)


if __name__ == "__main__":
    main()
