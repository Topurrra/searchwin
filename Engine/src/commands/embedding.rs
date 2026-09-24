//! Text embeddings for semantic search.
//!
//! Wraps a bundled all-MiniLM-L6-v2 ONNX model (Apache-2.0, ~23 MB int8)
//! via `fastembed`. The model is loaded lazily on first use and reused for
//! the process lifetime. MiniLM is a *symmetric* encoder — queries and
//! documents embed identically, no "query:"/"passage:" prefixes — so there
//! is one `embed`/`embed_batch` for both sides.
//!
//! Gating mirrors the other native-runtime features (`vosk`, `ocr-leptess`,
//! `screenrec`): the real implementation is behind the `semantic` cargo
//! feature, which pulls `fastembed` → `ort` (the ONNX Runtime). Builds
//! without the feature get inert stubs (`embed` → `None`, `is_available`
//! → `false`), so semantic search is simply absent, never a build break.
//!
//! Everything here is offline: the model + tokenizer are read from the
//! bundled resource dir (`embedding-runtime/`, set via `set_model_dir` at
//! startup). No HuggingFace download, no network.
//!
//! `chunk_text` (below) splits a document into overlapping passages so each
//! is embedded separately — a long document's specific matching passage is
//! not diluted into one averaged doc vector. It's pure logic (no model), so
//! it's available and tested even in builds without the `semantic` feature.

use std::path::PathBuf;
use std::sync::OnceLock;

/// all-MiniLM-L6-v2 output dimensionality.
pub const EMBED_DIM: usize = 384;

/// Absolute path to the bundled model dir (`embedding-runtime/`), resolved
/// from the Tauri resource dir once at startup. `embed` reads the ONNX +
/// tokenizer files from here.
static MODEL_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Record where the bundled model lives. Called once from app setup with
/// `app.path().resource_dir().join("embedding-runtime")`. Idempotent — a
/// second call is ignored.
pub fn set_model_dir(dir: PathBuf) {
    let _ = MODEL_DIR.set(dir);
}

/// Split a document into overlapping word-window passages for per-chunk
/// embedding. `target_words` sizes each window near the model's sequence
/// limit; `overlap` re-includes trailing words so a match spanning a chunk
/// boundary isn't lost. If the document yields more than `max_chunks`
/// windows, the windows are evenly subsampled across the whole document
/// (not just the head) so long documents stay represented end-to-end while
/// the embedding cost stays bounded.
///
/// ponytail: whitespace word-splitting, not real tokenization — close
/// enough for chunk sizing, and it never over-runs the model (fastembed
/// truncates internally). Upgrade to token-accurate windows if recall on
/// CJK / no-space scripts matters.
pub fn chunk_text(text: &str, target_words: usize, overlap: usize, max_chunks: usize) -> Vec<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.is_empty() || target_words == 0 || max_chunks == 0 {
        return Vec::new();
    }
    if words.len() <= target_words {
        return vec![words.join(" ")];
    }
    let step = target_words.saturating_sub(overlap).max(1);
    // Build every window first, then subsample if there are too many.
    let mut starts: Vec<usize> = (0..words.len()).step_by(step).collect();
    // Drop a final window that would be almost entirely overlap with the prior.
    if starts.len() >= 2 {
        let last = *starts.last().unwrap();
        if words.len().saturating_sub(last) <= overlap {
            starts.pop();
        }
    }
    if starts.len() > max_chunks {
        // Evenly spaced across the document, always including the first window.
        let n = starts.len();
        let picked: Vec<usize> = (0..max_chunks)
            .map(|i| starts[i * (n - 1) / (max_chunks - 1).max(1)])
            .collect();
        starts = picked;
        starts.dedup();
    }
    starts
        .into_iter()
        .map(|start| {
            let end = (start + target_words).min(words.len());
            words[start..end].join(" ")
        })
        .collect()
}

#[cfg(feature = "semantic")]
mod imp {
    use super::{EMBED_DIM, MODEL_DIR};
    use fastembed::{InitOptionsUserDefined, TextEmbedding, TokenizerFiles, UserDefinedEmbeddingModel};
    use std::fs;
    use std::sync::{LazyLock, Mutex};

    /// Loaded model, behind a Mutex (fastembed's session isn't `Sync`). The
    /// outer `Option` is `None` when loading failed (missing files, bad
    /// model) — we try once and cache the failure so we don't thrash on
    /// every query.
    static MODEL: LazyLock<Mutex<Option<TextEmbedding>>> =
        LazyLock::new(|| Mutex::new(load_model()));

    /// Read the bundled files and build a user-defined (offline) model.
    fn load_model() -> Option<TextEmbedding> {
        let dir = MODEL_DIR.get()?;
        let onnx = fs::read(dir.join("model.onnx")).ok()?;
        let tokenizer_files = TokenizerFiles {
            tokenizer_file: fs::read(dir.join("tokenizer.json")).ok()?,
            config_file: fs::read(dir.join("config.json")).ok()?,
            special_tokens_map_file: fs::read(dir.join("special_tokens_map.json")).ok()?,
            tokenizer_config_file: fs::read(dir.join("tokenizer_config.json")).ok()?,
        };
        let model = UserDefinedEmbeddingModel::new(onnx, tokenizer_files);
        match TextEmbedding::try_new_from_user_defined(model, InitOptionsUserDefined::default()) {
            Ok(embedder) => Some(embedder),
            Err(error) => {
                eprintln!("embedding: failed to load MiniLM model ({error}); semantic search disabled.");
                None
            }
        }
    }

    pub fn is_available() -> bool {
        MODEL.lock().map(|m| m.is_some()).unwrap_or(false)
    }

    pub fn embed_batch(texts: &[&str]) -> Option<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Some(Vec::new());
        }
        let mut guard = MODEL.lock().ok()?;
        let model = guard.as_mut()?;
        // fastembed batches internally; the model truncates each input to its
        // max sequence length, so long chunks are safe.
        let out = model.embed(texts.to_vec(), None).ok()?;
        // Guard the dimensionality once — a wrong model would silently poison
        // every cosine score otherwise.
        if out.iter().any(|v| v.len() != EMBED_DIM) {
            return None;
        }
        Some(out)
    }
}

#[cfg(not(feature = "semantic"))]
mod imp {
    // Inert stubs: semantic search is a no-op when the feature is off.
    pub fn is_available() -> bool {
        false
    }
    pub fn embed_batch(_texts: &[&str]) -> Option<Vec<Vec<f32>>> {
        None
    }
}

/// Whether semantic embedding is compiled in AND the model loaded.
pub fn is_available() -> bool {
    imp::is_available()
}

/// Embed one piece of text (query side) to a MiniLM vector, or `None` if
/// semantic search is unavailable or the text is empty. Never panics.
pub fn embed(text: &str) -> Option<Vec<f32>> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    imp::embed_batch(&[trimmed]).and_then(|mut v| v.pop())
}

/// Embed many passages at once (index side). Returns one vector per input in
/// order, or `None` if semantic search is unavailable. Empty-after-trim
/// inputs are dropped, so the output length may be shorter than the input —
/// callers pair vectors with chunks by filtering the same way.
pub fn embed_batch(texts: &[String]) -> Option<Vec<Vec<f32>>> {
    let trimmed: Vec<&str> = texts
        .iter()
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .collect();
    if trimmed.is_empty() {
        return Some(Vec::new());
    }
    imp::embed_batch(&trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_text_is_one_chunk() {
        let chunks = chunk_text("just a few words here", 180, 30, 16);
        assert_eq!(chunks, vec!["just a few words here".to_string()]);
        assert!(chunk_text("", 180, 30, 16).is_empty());
    }

    #[test]
    fn long_text_windows_with_overlap_and_cap() {
        // 1000 words, 100-word windows, 20 overlap → many windows, capped at 8.
        let text = (0..1000).map(|i| format!("w{i}")).collect::<Vec<_>>().join(" ");
        let chunks = chunk_text(&text, 100, 20, 8);
        assert!(!chunks.is_empty() && chunks.len() <= 8, "cap respected: {}", chunks.len());
        // First chunk starts at the document head.
        assert!(chunks[0].starts_with("w0 "));
        // Coverage reaches the tail (subsampled windows span the whole doc).
        assert!(chunks.last().unwrap().contains("w999"));
    }

    /// Loads the REAL bundled model and embeds text. Ignored by default (needs
    /// `embedding-runtime/` populated + the `semantic` feature); run with:
    ///
    /// ```text
    /// cargo test --features semantic -- --ignored loads_the_bundled_model
    /// ```
    ///
    /// Why this exists: every other test here is pure string logic, so the whole
    /// ONNX path had NO coverage — and `load_model` fails *silently* (it eprintln's
    /// and returns `None`, which just disables semantic search). A bad or wrongly
    /// quantized `model.onnx` therefore passed the entire suite while shipping a
    /// dead feature. Added 2026-07-21 when swapping the fp32 weights for int8.
    #[cfg(feature = "semantic")]
    #[test]
    #[ignore = "loads the real bundled ONNX model; run with --ignored"]
    fn loads_the_bundled_model_and_embeds_sensibly() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("embedding-runtime");
        assert!(dir.join("model.onnx").exists(), "no model at {}", dir.display());
        set_model_dir(dir);

        // The load itself is the primary assertion — this is what silently fails.
        assert!(is_available(), "model failed to load (semantic search would be silently OFF)");

        let vecs = embed_batch(&[
            "a small cat sat on the mat".to_string(),
            "a kitten rested on the rug".to_string(),
            "quarterly submarine propulsion economics".to_string(),
        ])
        .expect("embed_batch returned None");
        assert_eq!(vecs.len(), 3);
        for v in &vecs {
            assert_eq!(v.len(), EMBED_DIM, "wrong dimensionality");
            assert!(v.iter().all(|x| x.is_finite()), "non-finite values in embedding");
            assert!(v.iter().any(|&x| x != 0.0), "all-zero embedding");
        }

        // Semantic sanity: quantization must not destroy meaning. The cat/kitten
        // pair has to beat the cat/submarine pair by a clear margin.
        let cos = |a: &Vec<f32>, b: &Vec<f32>| -> f32 {
            let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
            let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
            let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
            dot / (na * nb)
        };
        let related = cos(&vecs[0], &vecs[1]);
        let unrelated = cos(&vecs[0], &vecs[2]);
        assert!(
            related > unrelated + 0.15,
            "quantization degraded meaning: related={related:.3} unrelated={unrelated:.3}"
        );
    }
}
