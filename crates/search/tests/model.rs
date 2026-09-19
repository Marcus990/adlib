//! Needs the downloaded model; run with `cargo test --release -p ls-search -- --ignored`.
use ls_search::Clip;
use std::path::Path;

/// Candle's OpenCLIP text encoder has no padding mask: batching padded phrases changed embeddings
/// (cos 0.963 vs single). So `best_match` embeds phrases one at a time. This test documents that
/// single embeddings are deterministic, and guards the per-phrase path.
#[test]
#[ignore]
fn single_text_embedding_is_stable_and_normalised() {
    let clip = Clip::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../models/mobileclip-s2")).unwrap();
    let a = clip.embed_text("a photo of a golden eagle").unwrap();
    let b = clip.embed_text("a photo of a golden eagle").unwrap();
    let norm: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let cos: f32 = a.iter().zip(&b).map(|(x, y)| x * y).sum();
    assert!((norm - 1.0).abs() < 1e-4 && cos > 0.99999, "norm {norm} cos {cos}");
}
