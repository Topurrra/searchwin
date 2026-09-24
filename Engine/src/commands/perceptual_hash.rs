//! Quality Pass Wave 1 / DF-1 (2026-05-29): perceptual image hashing
//! for DuplicateFinder's "Similar images" mode.
//!
//! Byte-identical dedup (Wave 8.5 BLAKE3 cache + the existing
//! DuplicateFinder pipeline) only catches files where every bit
//! matches. Anything that's been re-encoded, resized, lightly cropped,
//! rotated, or shared through a messaging app (which strips EXIF and
//! re-saves at a different quality) slips through.
//!
//! Perceptual hashing fixes that. We render every image down to a
//! small grayscale grid (8×8 by default → 64-bit fingerprint), then
//! group images whose fingerprints differ by ≤ N bits (Hamming
//! distance). That distance is the user-tunable "sensitivity"
//! threshold.
//!
//! Algorithm chosen: **dHash (gradient)**. Faster than pHash (no DCT),
//! more robust than aHash (mean), and gives a clean 64-bit value out.
//! Robust to scaling, brightness shifts, format changes, and JPEG
//! re-encodes. Less robust to large rotations (a 90° turn breaks it —
//! that's the trade-off vs the more expensive pHash + four-rotation
//! comparison; revisit if users report a real-world miss).
//!
//! Local. Free. No "image AI" cloud round-trip. The whole pipeline runs
//! offline on the user's machine.

use std::path::Path;

use image_hasher::{HashAlg, HasherConfig};

/// One image's perceptual fingerprint, byte-packed into a u64. Two
/// images are "similar" when their hashes differ by ≤ threshold bits
/// (Hamming distance).
pub type PerceptualHash = u64;

/// File extensions DF-1 will consider. Matches the OCR-eligible set
/// from text_extract.rs so users get consistent "scanned image"
/// behavior across the two image-touching features.
pub fn is_image_eligible(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    let ext = ext.to_ascii_lowercase();
    matches!(
        ext.as_str(),
        "png" | "jpg" | "jpeg" | "tif" | "tiff" | "bmp" | "webp" | "gif"
    )
}

/// Compute the perceptual hash of an image file. Returns `None` on
/// any IO/decode error so the caller can silently skip un-decodable
/// files (corrupt JPEGs, HEIC without codec, etc.) without failing
/// the whole scan.
pub fn compute_perceptual_hash(path: &Path) -> Option<PerceptualHash> {
    // dHash 8×8 = 8 columns × 8 rows of gradient comparisons → 64 bits.
    // Mean-prefiltered for stability under brightness shifts.
    let hasher = HasherConfig::new()
        .hash_alg(HashAlg::Gradient)
        .hash_size(8, 8)
        .preproc_dct() // Cheap DCT preprocess — extra stability vs JPEG re-encodes.
        .to_hasher();

    let img = image::open(path).ok()?;
    let hash = hasher.hash_image(&img);
    let bytes = hash.as_bytes();
    if bytes.len() < 8 {
        return None;
    }
    // Pack the first 8 bytes (= 64 bits) of the hash into a u64. The
    // image_hasher crate uses little-endian byte ordering internally.
    let mut buf = [0u8; 8];
    buf.copy_from_slice(&bytes[..8]);
    Some(u64::from_le_bytes(buf))
}

/// Hamming distance between two perceptual hashes — number of bits
/// that differ. Range 0 (identical fingerprint) to 64 (maximally
/// different). Most users tune the threshold around 4–10 for
/// "obvious duplicates" or 12–18 for "looser similarity matches".
#[inline]
pub fn hamming_distance(a: PerceptualHash, b: PerceptualHash) -> u32 {
    (a ^ b).count_ones()
}

/// Default sensitivity threshold — bits-of-difference at or below
/// which two images count as "similar". 10 is a conservative default:
/// catches obvious re-encodes / resizes without flooding the result
/// list with false positives.
pub const DEFAULT_SIMILARITY_THRESHOLD: u32 = 10;

/// One group of visually similar images. The frontend renders each
/// group with thumbnails (Tauri asset:// protocol) and a "keep"
/// heuristic recommendation per group.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SimilarImageGroup {
    /// Representative hash for the group (the first member's hash).
    /// Useful for cache / debug; not displayed in UI.
    pub representative_hash: String,
    /// Paths in this group. Order = scan order; the auto-keep
    /// heuristic on the frontend re-sorts them before display.
    pub paths: Vec<String>,
    /// Sizes (in bytes) parallel to `paths` so the UI can show "1.2
    /// MB / 4.5 MB / 800 KB" per row without a second stat round
    /// trip. (Computed cheaply during the scan.)
    pub sizes: Vec<u64>,
}

/// Union-Find / DSU over images so we can collapse pair-wise similar
/// hits into proper transitive groups. With N images and at most P
/// "similar" pairs (P ≪ N²), this stays O((N + P) α(N)).
pub struct SimilarityGrouper {
    parent: Vec<usize>,
    rank: Vec<u8>,
}

impl SimilarityGrouper {
    pub fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
            rank: vec![0; n],
        }
    }

    pub fn find(&mut self, x: usize) -> usize {
        let mut root = x;
        while self.parent[root] != root {
            root = self.parent[root];
        }
        // Path compression — flatten on the way back.
        let mut cur = x;
        while self.parent[cur] != root {
            let next = self.parent[cur];
            self.parent[cur] = root;
            cur = next;
        }
        root
    }

    pub fn union(&mut self, a: usize, b: usize) {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra == rb {
            return;
        }
        // Union by rank.
        if self.rank[ra] < self.rank[rb] {
            self.parent[ra] = rb;
        } else if self.rank[ra] > self.rank[rb] {
            self.parent[rb] = ra;
        } else {
            self.parent[rb] = ra;
            self.rank[ra] += 1;
        }
    }
}

/// Build groups of similar images from a flat list of (path, size,
/// hash) records and a Hamming-distance threshold. v1 implementation
/// is O(N²) in the hash-comparison step, which is fine up to ~50 K
/// images (a few seconds even on cold cache); beyond that, swap in a
/// BK-tree or VP-tree.
pub fn group_similar_images(
    records: &[(String, u64, PerceptualHash)],
    threshold: u32,
) -> Vec<SimilarImageGroup> {
    let n = records.len();
    if n == 0 {
        return Vec::new();
    }
    let mut dsu = SimilarityGrouper::new(n);

    // Pair-wise hamming. Skip the upper triangle (j < i would be a
    // duplicate comparison).
    for i in 0..n {
        let hi = records[i].2;
        for j in (i + 1)..n {
            let hj = records[j].2;
            if hamming_distance(hi, hj) <= threshold {
                dsu.union(i, j);
            }
        }
    }

    // Collect each root's members; drop singletons (no group needed).
    let mut groups_map: std::collections::HashMap<usize, Vec<usize>> =
        std::collections::HashMap::new();
    for i in 0..n {
        let root = dsu.find(i);
        groups_map.entry(root).or_default().push(i);
    }

    let mut out: Vec<SimilarImageGroup> = groups_map
        .into_values()
        .filter(|members| members.len() > 1)
        .map(|members| {
            // representative_hash is just the first member's hash as
            // hex — primarily for debugging / log analysis.
            let rep_hash = records[members[0]].2;
            SimilarImageGroup {
                representative_hash: format!("{:016x}", rep_hash),
                paths: members.iter().map(|&i| records[i].0.clone()).collect(),
                sizes: members.iter().map(|&i| records[i].1).collect(),
            }
        })
        .collect();

    // Largest groups first — typical "I have 5 copies of this photo"
    // wins more reclaimable space than "I have 2 copies of that".
    out.sort_by(|a, b| b.paths.len().cmp(&a.paths.len()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hamming_distance_is_symmetric_and_self_zero() {
        let a: PerceptualHash = 0xDEADBEEF_FEEDFACE;
        let b: PerceptualHash = 0xDEADBEEF_FEED0000;
        assert_eq!(hamming_distance(a, a), 0);
        assert_eq!(hamming_distance(a, b), hamming_distance(b, a));
        // 0xFACE (1111_1010_1100_1110) has 11 set bits, so it differs from
        // 0x0000 in all 11 of them.
        assert_eq!(hamming_distance(0xFACE, 0x0000), 11);
    }

    #[test]
    fn grouper_unions_transitively() {
        let mut dsu = SimilarityGrouper::new(5);
        dsu.union(0, 1);
        dsu.union(1, 2);
        dsu.union(3, 4);
        assert_eq!(dsu.find(0), dsu.find(2));
        assert_ne!(dsu.find(0), dsu.find(3));
    }

    #[test]
    fn group_similar_collapses_pairs() {
        let records = vec![
            ("a.jpg".to_string(), 100, 0b1111_1111_0000_0000u64),
            ("b.jpg".to_string(), 100, 0b1111_1111_0000_0001u64), // 1 bit off
            ("c.jpg".to_string(), 100, 0b1111_1111_0000_0011u64), // 2 bits off from a
            ("z.jpg".to_string(), 100, 0xFFFF_FFFF_FFFF_FFFFu64),
        ];
        let groups = group_similar_images(&records, 3);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].paths.len(), 3);
    }
}
