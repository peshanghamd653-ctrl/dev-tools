//! Vector math and storage codec for the semantic half of retrieval.
//!
//! Deliberately dependency-free: embeddings are plain `f32` little-endian
//! BLOBs and similarity is a brute-force scan in Rust. See the module docs
//! in `lib.rs` for why this beat `sqlite-vec` here.

use std::collections::HashMap;
use std::hash::Hash;

/// Reciprocal-rank-fusion damping constant. 60 is the value from the
/// original RRF paper and the one every implementation that doesn't tune
/// per-corpus uses; it keeps the top few ranks from dominating outright.
pub const RRF_K: f64 = 60.0;

/// Cosine similarity in `[-1.0, 1.0]`.
///
/// Returns `0.0` — "unrelated", the neutral answer — rather than `NaN` when
/// the vectors disagree on dimension or either one is all zeros, so a model
/// swap mid-index can never poison a ranking with `NaN` comparisons.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f64;
    let mut norm_a = 0.0f64;
    let mut norm_b = 0.0f64;
    for (x, y) in a.iter().zip(b.iter()) {
        dot += f64::from(*x) * f64::from(*y);
        norm_a += f64::from(*x) * f64::from(*x);
        norm_b += f64::from(*y) * f64::from(*y);
    }
    if norm_a <= 0.0 || norm_b <= 0.0 {
        return 0.0;
    }
    (dot / (norm_a.sqrt() * norm_b.sqrt())) as f32
}

/// Pack a vector as little-endian `f32`s — the same layout `sqlite-vec`
/// expects, so stored BLOBs stay readable if a real vector index lands later.
pub fn encode_vector(vector: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(vector.len() * 4);
    for value in vector {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out
}

/// Inverse of [`encode_vector`]. `None` on a truncated/corrupt BLOB rather
/// than a panic — a bad row should drop out of the ranking, not crash search.
pub fn decode_vector(bytes: &[u8]) -> Option<Vec<f32>> {
    if bytes.is_empty() || !bytes.len().is_multiple_of(4) {
        return None;
    }
    Some(
        bytes
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect(),
    )
}

/// Merge several rankings into one by reciprocal-rank fusion:
/// `score(d) = Σ 1/(k + rank(d))` over the lists that contain `d`, with
/// `rank` 1-based.
///
/// RRF only reads *positions*, never the underlying scores, which is exactly
/// what makes it usable here: bm25 (unbounded, lower-is-better) and cosine
/// (`[-1, 1]`, higher-is-better) never have to be normalized against each
/// other, and no weight needs tuning. Ties keep first-seen order, so a
/// lexical-only result set comes back in bm25 order unchanged.
pub fn reciprocal_rank_fusion<T: Eq + Hash + Clone>(rankings: &[Vec<T>], k: f64) -> Vec<(T, f64)> {
    let mut order: Vec<T> = Vec::new();
    let mut seen: HashMap<T, usize> = HashMap::new();
    let mut scores: Vec<f64> = Vec::new();

    for ranking in rankings {
        for (position, item) in ranking.iter().enumerate() {
            let contribution = 1.0 / (k + (position as f64) + 1.0);
            match seen.get(item) {
                Some(&slot) => scores[slot] += contribution,
                None => {
                    seen.insert(item.clone(), order.len());
                    order.push(item.clone());
                    scores.push(contribution);
                }
            }
        }
    }

    let mut fused: Vec<(T, f64)> = order.into_iter().zip(scores).collect();
    // `sort_by` is stable, so equal scores keep first-seen order.
    fused.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    fused
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine_identical_orthogonal_and_opposite() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0];
        let c = vec![-1.0, 0.0, 0.0];

        // Identical vectors: exactly 1.
        assert!((cosine_similarity(&a, &a) - 1.0).abs() < 1e-6);
        // Orthogonal: exactly 0.
        assert!(cosine_similarity(&a, &b).abs() < 1e-6);
        // Opposite: exactly -1.
        assert!((cosine_similarity(&a, &c) + 1.0).abs() < 1e-6);

        // Magnitude must not matter — only direction.
        let scaled = vec![7.5, 0.0, 0.0];
        assert!((cosine_similarity(&a, &scaled) - 1.0).abs() < 1e-6);

        // A known non-trivial value: cos between (1,1) and (1,0) is 1/√2.
        let d = vec![1.0, 1.0];
        let e = vec![1.0, 0.0];
        assert!((cosine_similarity(&d, &e) - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
    }

    #[test]
    fn cosine_degenerate_inputs_are_zero_not_nan() {
        // Dimension mismatch (a model swap mid-index) must not produce NaN.
        assert_eq!(cosine_similarity(&[1.0, 0.0], &[1.0, 0.0, 0.0]), 0.0);
        // Zero vectors have no direction.
        assert_eq!(cosine_similarity(&[0.0, 0.0], &[1.0, 1.0]), 0.0);
        assert_eq!(cosine_similarity(&[], &[]), 0.0);
    }

    #[test]
    fn vector_blob_roundtrip() {
        let v = vec![0.0, -1.5, 3.25, f32::MIN_POSITIVE];
        let bytes = encode_vector(&v);
        assert_eq!(bytes.len(), v.len() * 4);
        assert_eq!(decode_vector(&bytes).unwrap(), v);

        // Corrupt/truncated blobs decode to None instead of panicking.
        assert_eq!(decode_vector(&bytes[..bytes.len() - 1]), None);
        assert_eq!(decode_vector(&[]), None);
    }

    #[test]
    fn rrf_merges_two_known_rankings() {
        // With k = 60 and 1-based ranks:
        //   b = 1/62 + 1/61 = 0.032522   (2nd lexically, 1st semantically)
        //   a = 1/61 + 1/63 = 0.032266   (1st lexically, 3rd semantically)
        //   d = 1/62        = 0.016129   (semantic only, rank 2)
        //   c = 1/63        = 0.015873   (lexical only, rank 3)
        // So "b" overtakes the top lexical hit, and "d" — which lexical
        // search never found at all — outranks the weakest lexical hit.
        let lexical = vec!["a", "b", "c"];
        let semantic = vec!["b", "d", "a"];
        let fused = reciprocal_rank_fusion(&[lexical, semantic], RRF_K);

        let order: Vec<&str> = fused.iter().map(|(item, _)| *item).collect();
        assert_eq!(order, vec!["b", "a", "d", "c"]);

        // Items in both lists must outscore items in only one.
        let score = |name: &str| fused.iter().find(|(i, _)| *i == name).unwrap().1;
        assert!(score("b") > score("c"));
        assert!(score("a") > score("d"));
        assert!((score("b") - (1.0 / 62.0 + 1.0 / 61.0)).abs() < 1e-12);
        assert!((score("c") - (1.0 / 63.0)).abs() < 1e-12);
    }

    #[test]
    fn rrf_with_one_ranking_preserves_its_order() {
        // The degradation path: no semantic list, so bm25 order survives.
        let lexical = vec!["a", "b", "c"];
        let fused = reciprocal_rank_fusion(&[lexical], RRF_K);
        let order: Vec<&str> = fused.iter().map(|(item, _)| *item).collect();
        assert_eq!(order, vec!["a", "b", "c"]);
        assert!(reciprocal_rank_fusion::<&str>(&[], RRF_K).is_empty());
    }
}
