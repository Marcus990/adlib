//! Track B: local image search (design doc §7.3). MobileCLIP v1 S2 via Candle on the CPU
//! (Candle's Metal path for FastViT measured ~170 s/image and a batch-8 Metal run OOM'd the
//! 8 GB Mac — see PROGRESS.md). Brute-force cosine over a small in-RAM index; an in-memory
//! LRU image cache feeds the `img://` protocol.

use anyhow::{Context, Error as E, Result};
use candle_core::{DType, Device, Tensor, D};
use candle_nn::VarBuilder;
use candle_transformers::models::mobileclip;
use ls_contracts::Match;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokenizers::Tokenizer;

pub const MODEL_NAME: &str = "mobileclip-v1-s2";
pub const DIM: usize = 512;

pub struct Clip {
    model: mobileclip::MobileClipModel,
    tok: Tokenizer,
    dev: Device,
    image_size: usize,
}

impl Clip {
    /// `dir` holds `open_clip_model.safetensors` and `tokenizer.json` (apple/MobileCLIP-S2-OpenCLIP).
    pub fn load(dir: &Path) -> Result<Self> {
        let dev = Device::Cpu;
        let cfg = mobileclip::MobileClipConfig::s2();
        let vb = unsafe {
            VarBuilder::from_mmaped_safetensors(&[dir.join("open_clip_model.safetensors")], DType::F32, &dev)?
        };
        let model = mobileclip::MobileClipModel::new(vb, &cfg)?;
        let tok = Tokenizer::from_file(dir.join("tokenizer.json")).map_err(E::msg)?;
        Ok(Self { model, tok, dev, image_size: cfg.image_size })
    }

    pub fn embed_text(&self, s: &str) -> Result<Vec<f32>> {
        let mut ids = self.tok.encode(s, true).map_err(E::msg)?.get_ids().to_vec();
        ids.truncate(77); // CLIP context length
        let input = Tensor::new(vec![ids], &self.dev)?;
        let v = l2(&self.model.get_text_features(&input)?)?;
        Ok(v.flatten_all()?.to_vec1::<f32>()?)
    }

    pub fn embed_image(&self, path: &Path) -> Result<Vec<f32>> {
        let img = image::ImageReader::open(path)?.with_guessed_format()?.decode()?;
        let s = self.image_size as u32;
        let img = img.resize_to_fill(s, s, image::imageops::FilterType::Triangle);
        let data = img.to_rgb8().into_raw();
        let t = Tensor::from_vec(data, (self.image_size, self.image_size, 3), &self.dev)?.permute((2, 0, 1))?;
        let t = (t.to_dtype(DType::F32)? / 255.0)?.unsqueeze(0)?;
        let v = l2(&self.model.get_image_features(&t)?)?;
        Ok(v.flatten_all()?.to_vec1::<f32>()?)
    }
}

fn l2(v: &Tensor) -> Result<Tensor> {
    Ok(v.broadcast_div(&v.sqr()?.sum_keepdim(D::Minus1)?.sqrt()?)?)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub file: String,
    pub caption: String,
    pub img: Vec<f32>,
    pub cap: Vec<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Index {
    pub model: String,
    /// Absolute path of the library folder the `file` fields are relative to.
    pub root: String,
    pub entries: Vec<Entry>,
}

impl Index {
    pub fn load(path: &Path) -> Result<Self> {
        let s = std::fs::read_to_string(path).with_context(|| format!("reading index {}", path.display()))?;
        let idx: Index = serde_json::from_str(&s)?;
        anyhow::ensure!(idx.model == MODEL_NAME, "index built with {}, expected {MODEL_NAME}; re-index", idx.model);
        Ok(idx)
    }
    pub fn captions(&self) -> Vec<String> {
        self.entries.iter().map(|e| e.caption.clone()).collect()
    }
    pub fn path_of(&self, id: &str) -> Option<PathBuf> {
        self.entries.iter().find(|e| e.id == id).map(|e| Path::new(&self.root).join(&e.file))
    }
}

/// Read `captions.tsv` (`id<TAB>caption`) if present; otherwise derive captions from filenames.
pub fn library_files(dir: &Path) -> Result<Vec<(String, String, String)>> {
    let mut caps: HashMap<String, String> = HashMap::new();
    if let Ok(tsv) = std::fs::read_to_string(dir.join("captions.tsv")) {
        for line in tsv.lines() {
            if let Some((id, cap)) = line.split_once('\t') {
                caps.insert(id.trim().to_string(), cap.trim().to_string());
            }
        }
    }
    let mut out = vec![];
    for e in std::fs::read_dir(dir)? {
        let p = e?.path();
        let ext = p.extension().and_then(|x| x.to_str()).unwrap_or("").to_lowercase();
        if !["jpg", "jpeg", "png", "webp"].contains(&ext.as_str()) {
            continue;
        }
        let id = p.file_stem().unwrap().to_string_lossy().to_string();
        let caption = caps.get(&id).cloned().unwrap_or_else(|| id.replace(['-', '_'], " "));
        out.push((id, p.file_name().unwrap().to_string_lossy().to_string(), caption));
    }
    out.sort();
    Ok(out)
}

/// Brute-force scorer over the index (design doc §7.3).
pub struct Searcher {
    pub index: Index,
    /// Optional CLIP prompt template, e.g. "a photo of {}". Chosen during calibration.
    pub template: Option<String>,
}

/// Best image for one phrase, plus the runner-up for logging.
#[derive(Debug, Clone)]
pub struct PhraseHit {
    pub phrase: String,
    pub id: String,
    pub caption: String,
    pub score: f32,
    pub runner_up: Option<(String, f32)>,
}

impl Searcher {
    pub fn new(index: Index) -> Self {
        Self { index, template: Some("a photo of {}".into()) }
    }

    pub fn score_vec(&self, q: &[f32]) -> Vec<f32> {
        self.index
            .entries
            .iter()
            .map(|e| 0.5 * (dot(&e.img, q) + dot(&e.cap, q)))
            .collect()
    }

    pub fn hit_for(&self, phrase: &str, q: &[f32]) -> Option<PhraseHit> {
        let scores = self.score_vec(q);
        let mut idx: Vec<usize> = (0..scores.len()).collect();
        idx.sort_by(|a, b| scores[*b].partial_cmp(&scores[*a]).unwrap_or(std::cmp::Ordering::Equal));
        let first = *idx.first()?;
        let e = &self.index.entries[first];
        Some(PhraseHit {
            phrase: phrase.to_string(),
            id: e.id.clone(),
            caption: e.caption.clone(),
            score: scores[first],
            runner_up: idx.get(1).map(|i| (self.index.entries[*i].id.clone(), scores[*i])),
        })
    }

    pub fn query_text(&self, phrase: &str) -> String {
        match &self.template {
            Some(t) => t.replace("{}", phrase),
            None => phrase.to_string(),
        }
    }

    /// First phrase's best image, unless another phrase's best scores ≥ 0.05 higher (§7.3).
    /// τ is applied later by the stage, so a weak best is still returned (and logged).
    pub fn best_match(&self, clip: &Clip, chunk_id: u64, phrases: &[String]) -> Result<(Option<Match>, Vec<PhraseHit>)> {
        self.best_match_avoiding(clip, chunk_id, phrases, None)
    }

    /// Like `best_match`, but a secondary phrase may not win by pointing at `on_screen` (the query
    /// model tends to repeat the on-screen subject; live test: ["panther", "white rose"] → white rose).
    pub fn best_match_avoiding(&self, clip: &Clip, chunk_id: u64, phrases: &[String], on_screen: Option<&str>) -> Result<(Option<Match>, Vec<PhraseHit>)> {
        let mut hits = vec![];
        for p in phrases.iter().take(3) {
            let q = clip.embed_text(&self.query_text(p))?;
            if let Some(h) = self.hit_for(p, &q) {
                hits.push(h);
            }
        }
        Ok((pick_avoiding(&hits, on_screen).map(|h| Match {
            chunk_id,
            image_id: h.id.clone(),
            caption: h.caption.clone(),
            score: h.score,
            phrase: h.phrase.clone(),
        }), hits))
    }
}

pub fn pick(hits: &[PhraseHit]) -> Option<&PhraseHit> {
    pick_avoiding(hits, None)
}

pub fn pick_avoiding<'a>(hits: &'a [PhraseHit], on_screen: Option<&str>) -> Option<&'a PhraseHit> {
    let first = hits.first()?;
    let best_other = hits[1..]
        .iter()
        .filter(|h| Some(h.id.as_str()) != on_screen)
        .max_by(|a, b| a.score.partial_cmp(&b.score).unwrap());
    match best_other {
        Some(o) if o.score + 1e-6 >= first.score + 0.05 => Some(o),
        _ => Some(first),
    }
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Byte-bounded LRU of image files, shared with the `img://` protocol handler.
#[derive(Clone)]
pub struct ImageCache {
    inner: Arc<Mutex<CacheInner>>,
}

struct CacheInner {
    map: HashMap<String, Arc<Vec<u8>>>,
    order: VecDeque<String>,
    bytes: usize,
    cap_bytes: usize,
    paths: HashMap<String, PathBuf>,
    pub hits: u64,
    pub misses: u64,
}

impl ImageCache {
    pub fn new(index: &Index, cap_bytes: usize) -> Self {
        let paths = index.entries.iter().map(|e| (e.id.clone(), Path::new(&index.root).join(&e.file))).collect();
        Self {
            inner: Arc::new(Mutex::new(CacheInner {
                map: HashMap::new(),
                order: VecDeque::new(),
                bytes: 0,
                cap_bytes,
                paths,
                hits: 0,
                misses: 0,
            })),
        }
    }

    /// Load every image (up to the byte cap) so the talk never waits on the card.
    pub fn prefetch_all(&self) -> usize {
        let ids: Vec<String> = self.inner.lock().unwrap().paths.keys().cloned().collect();
        ids.iter().filter(|id| self.get(id).is_some()).count()
    }

    pub fn get(&self, id: &str) -> Option<Arc<Vec<u8>>> {
        let mut c = self.inner.lock().unwrap();
        if let Some(b) = c.map.get(id).cloned() {
            c.hits += 1;
            c.order.retain(|x| x != id);
            c.order.push_back(id.to_string());
            return Some(b);
        }
        c.misses += 1;
        let path = c.paths.get(id)?.clone();
        let bytes = Arc::new(std::fs::read(&path).ok()?);
        c.bytes += bytes.len();
        c.map.insert(id.to_string(), bytes.clone());
        c.order.push_back(id.to_string());
        while c.bytes > c.cap_bytes && c.order.len() > 1 {
            if let Some(old) = c.order.pop_front() {
                if let Some(b) = c.map.remove(&old) {
                    c.bytes -= b.len();
                }
            }
        }
        Some(bytes)
    }

    pub fn stats(&self) -> (u64, u64, usize) {
        let c = self.inner.lock().unwrap();
        (c.hits, c.misses, c.bytes)
    }

    pub fn mime_of(id_or_path: &str) -> &'static str {
        let l = id_or_path.to_lowercase();
        if l.ends_with(".png") { "image/png" } else if l.ends_with(".webp") { "image/webp" } else { "image/jpeg" }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, img: Vec<f32>, cap: Vec<f32>) -> Entry {
        Entry { id: id.into(), file: format!("{id}.jpg"), caption: id.into(), img, cap }
    }

    #[test]
    fn scoring_is_mean_of_image_and_caption() {
        let idx = Index {
            model: MODEL_NAME.into(),
            root: "/tmp".into(),
            entries: vec![entry("a", vec![1.0, 0.0], vec![0.0, 1.0]), entry("b", vec![1.0, 0.0], vec![1.0, 0.0])],
        };
        let s = Searcher::new(idx);
        assert_eq!(s.score_vec(&[1.0, 0.0]), vec![0.5, 1.0]);
        let h = s.hit_for("x", &[1.0, 0.0]).unwrap();
        assert_eq!((h.id.as_str(), h.runner_up.unwrap().0.as_str()), ("b", "a"));
    }

    #[test]
    fn pick_prefers_first_phrase_unless_clearly_beaten() {
        let h = |p: &str, s: f32| PhraseHit { phrase: p.into(), id: p.into(), caption: p.into(), score: s, runner_up: None };
        assert_eq!(pick(&[h("a", 0.30), h("b", 0.34)]).unwrap().id, "a");
        assert_eq!(pick(&[h("a", 0.30), h("b", 0.35)]).unwrap().id, "b");
        assert!(pick(&[]).is_none());
        // the on-screen image can't win through a secondary phrase
        assert_eq!(pick_avoiding(&[h("panther", 0.40), h("white-rose", 0.59)], Some("white-rose")).unwrap().id, "panther");
    }

    #[test]
    fn cache_evicts_by_bytes_and_counts_hits() {
        let dir = std::env::temp_dir().join(format!("ls-cache-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for (id, n) in [("x", 10usize), ("y", 10), ("z", 10)] {
            std::fs::write(dir.join(format!("{id}.jpg")), vec![0u8; n]).unwrap();
        }
        let idx = Index {
            model: MODEL_NAME.into(),
            root: dir.to_string_lossy().into(),
            entries: ["x", "y", "z"].iter().map(|i| entry(i, vec![], vec![])).collect(),
        };
        let c = ImageCache::new(&idx, 25);
        assert_eq!(c.get("x").unwrap().len(), 10);
        c.get("y");
        c.get("x"); // hit, x becomes most recent
        c.get("z"); // evicts y (least recent)
        let (hits, misses, bytes) = c.stats();
        assert_eq!((hits, misses, bytes), (1, 3, 20));
        assert!(c.get("nope").is_none());
    }
}
