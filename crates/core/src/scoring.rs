// AI-assisted
//! Weighted-sum score recalculation over pre-normalized `[0.0, 1.0]` site feature scores.
//!
//! Per `AGENTS.md` §3.1 and §5.4:
//! - Canonical scores are computed offline by `pipeline/build_catalog.py` from `pipeline/weights.json`.
//! - Rust never executes raw raster math or hardcodes scientific formulas/criteria.
//! - `analog_core::scoring` dynamically recalculates weighted sums over the feature keys
//!   present in `Catalog` and the researcher's custom weights.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use crate::models::{Catalog, RecalculateResponse, ScoredSite, SiteMetadata};

/// Sanitizes and normalizes a map of feature weights so that all non-negative finite weights
/// sum to `1.0`.
///
/// If all weights are zero, negative, or non-finite, all returned weights are `0.0`.
pub fn normalize_weights(raw_weights: &BTreeMap<String, f64>) -> BTreeMap<String, f64> {
    let mut sanitized: BTreeMap<String, f64> = BTreeMap::new();
    let mut sum = 0.0_f64;

    for (key, &val) in raw_weights {
        let clean = if val.is_finite() && val > 0.0 {
            val
        } else {
            0.0
        };
        sanitized.insert(key.clone(), clean);
        sum += clean;
    }

    if sum > 0.0 {
        for val in sanitized.values_mut() {
            *val /= sum;
        }
    }

    sanitized
}

/// Computes the weighted sum for a single site's pre-normalized `[0.0, 1.0]` feature scores.
///
/// Both `feature_scores` and `weights` are matched dynamically by key name so that no
/// scientific criteria are hardcoded in Rust.
pub fn compute_weighted_score(
    feature_scores: &BTreeMap<String, f64>,
    weights: &BTreeMap<String, f64>,
) -> f64 {
    let normalized = normalize_weights(weights);
    compute_with_normalized_weights(feature_scores, &normalized)
}

#[inline]
fn compute_with_normalized_weights(
    feature_scores: &BTreeMap<String, f64>,
    normalized_weights: &BTreeMap<String, f64>,
) -> f64 {
    let mut total = 0.0_f64;
    for (feature, &weight) in normalized_weights {
        if weight <= 0.0 {
            continue;
        }
        if let Some(&raw_score) = feature_scores.get(feature)
            && raw_score.is_finite()
        {
            let clamped_score = raw_score.clamp(0.0, 1.0);
            total += clamped_score * weight;
        }
    }
    total.clamp(0.0, 1.0)
}

/// Recalculates weighted scores for a slice of [`SiteMetadata`] and returns them sorted
/// from highest score to lowest score (with deterministic tie-breaking by `site_id`).
pub fn recalculate_sites(
    sites: &[SiteMetadata],
    custom_weights: &BTreeMap<String, f64>,
) -> Vec<ScoredSite> {
    let normalized = normalize_weights(custom_weights);
    let mut results: Vec<ScoredSite> = sites
        .iter()
        .map(|site| ScoredSite {
            site_id: site.id.clone(),
            name: site.name.clone(),
            body: site.body,
            score: compute_with_normalized_weights(&site.feature_scores, &normalized),
            canonical_score: site.canonical_score,
        })
        .collect();

    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(Ordering::Equal)
            .then_with(|| a.site_id.cmp(&b.site_id))
    });

    results
}

/// Recalculates all site scores in a [`Catalog`] using the provided researcher weights.
pub fn recalculate_catalog(
    catalog: &Catalog,
    custom_weights: &BTreeMap<String, f64>,
) -> RecalculateResponse {
    let normalized_weights = normalize_weights(custom_weights);
    let results = recalculate_sites(&catalog.sites, &normalized_weights);

    RecalculateResponse {
        results,
        normalized_weights,
        generated_by: Some("analog_core::scoring".to_string()),
    }
}
