//! Public channels and the set-reconciliation used to heal partitions.
//!
//! When two islands of the mesh rejoin, each side advertises which message digests it holds for
//! a channel (a `SyncFilter`); the other side replies with whatever the first is missing. For
//! M0 the filter is an explicit digest set (capped); a Golomb-coded set (GCS) is a later,
//! bandwidth-saving refinement that keeps the same interface.

/// Normalize a channel name: lower-cased, leading `#` implied. `"General"` and `"#general"`
/// address the same channel.
pub fn normalize(name: &str) -> String {
    let trimmed = name.trim().trim_start_matches('#');
    format!("#{}", trimmed.to_lowercase())
}

/// The default channel every node joins on first run.
pub const DEFAULT_CHANNEL: &str = "#general";

/// A compact statement of "these are the digests I hold for this channel."
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncFilter {
    pub channel: String,
    pub digests: Vec<[u8; 8]>,
}

impl SyncFilter {
    pub fn new(channel: impl Into<String>, mut digests: Vec<[u8; 8]>) -> Self {
        digests.sort_unstable();
        digests.dedup();
        Self {
            channel: channel.into(),
            digests,
        }
    }

    /// Digests present locally (`self`) that the remote filter is missing — i.e. what we should
    /// send them to bring them up to date.
    pub fn missing_from(&self, remote: &SyncFilter) -> Vec<[u8; 8]> {
        use std::collections::HashSet;
        let theirs: HashSet<_> = remote.digests.iter().copied().collect();
        self.digests
            .iter()
            .copied()
            .filter(|d| !theirs.contains(d))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_forms() {
        assert_eq!(normalize("General"), "#general");
        assert_eq!(normalize("#Medic"), "#medic");
        assert_eq!(normalize("  north-gate "), "#north-gate");
    }

    #[test]
    fn missing_computes_difference() {
        let mine = SyncFilter::new("#c", vec![[1; 8], [2; 8], [3; 8]]);
        let theirs = SyncFilter::new("#c", vec![[2; 8]]);
        let mut missing = mine.missing_from(&theirs);
        missing.sort();
        assert_eq!(missing, vec![[1; 8], [3; 8]]);
    }

    #[test]
    fn nothing_missing_when_in_sync() {
        let a = SyncFilter::new("#c", vec![[1; 8], [2; 8]]);
        let b = SyncFilter::new("#c", vec![[2; 8], [1; 8]]);
        assert!(a.missing_from(&b).is_empty());
    }
}
