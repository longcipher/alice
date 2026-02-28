//! Hybrid retrieval helpers.

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

use crate::memory::domain::HybridWeights;

/// Convert FTS5 BM25 rank to a score where larger is better.
#[must_use]
pub fn normalize_bm25_rank(rank: f32) -> f32 {
    let denominator = 1.0 + rank.abs();
    (1.0 / denominator).clamp(0.0, 1.0)
}

/// Fuse BM25 and vector signals.
#[must_use]
pub fn fuse_scores(bm25_score: f32, vector_score: Option<f32>, weights: HybridWeights) -> f32 {
    let vector = vector_score.unwrap_or(0.0).clamp(0.0, 1.0);
    let bm25 = bm25_score.clamp(0.0, 1.0);
    (weights.bm25.mul_add(bm25, weights.vector * vector)).clamp(0.0, 1.0)
}

/// Sanitize text for `fts5 MATCH` query use.
#[must_use]
pub fn sanitize_fts_query(input: &str) -> String {
    let cleaned: String = input
        .chars()
        .map(|ch| {
            if matches!(ch, '-' | '*' | '"' | '(' | ')' | '{' | '}' | ':' | '^' | '+' | '~' | '\\')
            {
                ' '
            } else {
                ch
            }
        })
        .collect();

    cleaned
        .split_whitespace()
        .filter(|token| !token.is_empty())
        .map(|token| format!("\"{token}\""))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Build a deterministic low-cost text embedding.
#[must_use]
pub fn simple_text_embedding(text: &str, dimensions: usize) -> Vec<f32> {
    let dims = dimensions.max(1);
    let mut output = vec![0.0_f32; dims];

    for token in text.split_whitespace().map(str::trim).filter(|token| token.len() >= 2) {
        let mut hasher = DefaultHasher::new();
        token.hash(&mut hasher);
        let hash = hasher.finish();
        let idx = (hash as usize) % dims;
        let sign = if (hash & 1) == 0 { 1.0 } else { -1.0 };
        output[idx] += sign;
    }

    let norm = output.iter().map(|value| value * value).sum::<f32>().sqrt();
    if norm > f32::EPSILON {
        for value in &mut output {
            *value /= norm;
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bm25_rank_normalization_is_bounded() {
        let value = normalize_bm25_rank(-0.5);
        assert!(value > 0.0);
        assert!(value <= 1.0);
    }

    #[test]
    fn sanitize_replaces_operators() {
        let sanitized = sanitize_fts_query("sqlite-vec + fts5");
        assert!(!sanitized.contains('-'));
        assert!(!sanitized.contains('+'));
    }

    #[test]
    fn simple_embedding_is_normalized() {
        let emb = simple_text_embedding("alpha beta beta", 32);
        let norm = emb.iter().map(|value| value * value).sum::<f32>().sqrt();
        if norm > f32::EPSILON {
            assert!((norm - 1.0).abs() < 0.001);
        }
    }
}
