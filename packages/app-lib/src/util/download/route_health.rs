//! Small, deterministic pieces of route health accounting.

const EWMA_ALPHA: f64 = 0.25;

/// Updates a metric using the same weighted moving average for every route.
pub(crate) fn update_ewma(current: &mut Option<f64>, sample: f64) {
    *current = Some(current.map_or(sample, |current| {
        current * (1.0 - EWMA_ALPHA) + sample * EWMA_ALPHA
    }));
}

/// Stable scope identifier for task-level route probes.
pub(crate) fn probe_scope(family: &str, authorities: &mut Vec<String>) -> u64 {
    use std::hash::{Hash, Hasher};
    authorities.sort_unstable();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    family.hash(&mut hasher);
    authorities.hash(&mut hasher);
    hasher.finish()
}
