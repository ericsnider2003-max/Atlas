//! Shared admission arithmetic for heavyweight local model engines, keeping
//! free-memory thresholds and recovery headroom consistent across voice and
//! reasoning runtimes without coupling their loaders together.

/// Return the required free-memory threshold when the measured amount is too
/// low. Unknown memory is intentionally admitted; callers decide whether a
/// provider can operate without a measurement.
pub(crate) fn below_threshold(
    free_mb: Option<u64>,
    typical_mb: u64,
    headroom_mb: u64,
) -> Option<(u64, u64)> {
    let required = typical_mb.saturating_add(headroom_mb);
    free_mb
        .filter(|free| *free < required)
        .map(|free| (free, required))
}

#[cfg(test)]
mod tests {
    use super::below_threshold;

    #[test]
    fn only_measured_low_memory_is_rejected() {
        assert_eq!(below_threshold(Some(799), 500, 300), Some((799, 800)));
        assert_eq!(below_threshold(Some(800), 500, 300), None);
        assert_eq!(below_threshold(None, 500, 300), None);
    }
}
