//! Round-robin rotation over one upstream's API keys.
//!
//! Extracted from the Tavily provider so every upstream that accepts more than
//! one key rotates the same way and agrees on which failures are the key's
//! fault. Behaviour is unchanged from the Tavily-private original.

use std::sync::atomic::{AtomicUsize, Ordering};

/// Round-robin ring over one or more API keys for a single upstream. Shared
/// (`Arc`) across provider clones so the rotation cursor is global: each
/// request starts on the next key, spreading credit consumption evenly across
/// all keys.
pub(crate) struct KeyRing {
    keys: Vec<String>,
    cursor: AtomicUsize,
}

impl KeyRing {
    /// Split a comma-separated key list into the ring. Whitespace around each
    /// segment is trimmed and empty segments are dropped, so `"a, b,"` yields
    /// two keys. When no non-empty segment remains (e.g. the raw value is
    /// empty), the raw value is kept as a single key — preserving the previous
    /// single-key behavior where a bogus key simply fails upstream with 401.
    pub(crate) fn parse(raw: &str) -> Self {
        let mut keys: Vec<String> = raw
            .split(',')
            .map(str::trim)
            .filter(|segment| !segment.is_empty())
            .map(str::to_string)
            .collect();
        if keys.is_empty() {
            keys.push(raw.to_string());
        }
        // Start at a random offset. On the HTTP transport the provider (and this
        // ring) is rebuilt per request, so a fixed start of 0 would make every
        // request begin on the first key and concentrate load there. A random
        // start spreads load across keys statelessly; stdio keeps its persistent
        // round-robin (the random start is just a one-time offset).
        let start = random_offset(keys.len());
        Self {
            keys,
            cursor: AtomicUsize::new(start),
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.keys.len()
    }

    /// Index the next request should start from. `Relaxed` is enough: the
    /// cursor only needs even distribution, not cross-request ordering.
    pub(crate) fn start(&self) -> usize {
        self.cursor.fetch_add(1, Ordering::Relaxed) % self.keys.len()
    }

    pub(crate) fn key(&self, index: usize) -> &str {
        &self.keys[index % self.keys.len()]
    }
}

/// A best-effort random starting offset in `0..len` (falls back to 0 if the RNG
/// is unavailable). Only meaningful for multi-key rings.
fn random_offset(len: usize) -> usize {
    if len <= 1 {
        return 0;
    }
    let mut buf = [0u8; 8];
    match getrandom::fill(&mut buf) {
        Ok(()) => (u64::from_ne_bytes(buf) % len as u64) as usize,
        Err(_) => 0,
    }
}

/// HTTP statuses that indict the *key* rather than the request or upstream:
/// 401/403 (invalid or unauthorized key), 429 (per-key rate limit), and
/// Tavily's 432 (plan limit exceeded) / 433 (pay-as-you-go limit exceeded).
/// Only these trigger rotation to the next key — timeouts and 5xx are
/// upstream-wide, so retrying with another key would just add latency.
///
/// The two Tavily-specific codes are inert for upstreams that never return
/// them, so one predicate serves every provider.
pub(crate) fn is_key_scoped_status(status: u16) -> bool {
    matches!(status, 401 | 403 | 429 | 432 | 433)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_key_parses_to_one_entry() {
        let ring = KeyRing::parse("tvly-only");
        assert_eq!(ring.len(), 1);
        assert_eq!(ring.key(0), "tvly-only");
    }

    #[test]
    fn comma_separated_keys_split_trim_and_drop_empties() {
        let ring = KeyRing::parse(" tvly-a, tvly-b ,, tvly-c,");
        assert_eq!(ring.len(), 3);
        assert_eq!(ring.key(0), "tvly-a");
        assert_eq!(ring.key(1), "tvly-b");
        assert_eq!(ring.key(2), "tvly-c");
    }

    #[test]
    fn all_empty_segments_fall_back_to_raw_value() {
        // Preserves the legacy single-key path: a degenerate value still
        // produces one key that fails upstream, instead of an empty ring.
        let ring = KeyRing::parse("");
        assert_eq!(ring.len(), 1);
        assert_eq!(ring.key(0), "");
    }

    #[test]
    fn start_rotates_round_robin_across_requests() {
        // The starting key is randomized per ring, but consecutive starts still
        // advance by one and wrap — round-robin is preserved.
        let ring = KeyRing::parse("a,b,c");
        let first = ring.start();
        assert_eq!(ring.start(), (first + 1) % 3);
        assert_eq!(ring.start(), (first + 2) % 3);
        assert_eq!(ring.start(), (first + 3) % 3);
    }

    #[test]
    fn single_key_ring_always_starts_at_zero() {
        // A single-key ring has a deterministic (0) start — no randomization.
        let ring = KeyRing::parse("solo");
        assert_eq!(ring.start(), 0);
        assert_eq!(ring.start(), 0);
    }

    #[test]
    fn key_indexing_wraps_for_failover_offsets() {
        let ring = KeyRing::parse("a,b");
        assert_eq!(ring.key(2), "a");
        assert_eq!(ring.key(3), "b");
    }

    #[test]
    fn only_key_scoped_statuses_rotate() {
        for status in [401, 403, 429, 432, 433] {
            assert!(is_key_scoped_status(status), "{status} indicts the key");
        }
        for status in [400, 404, 408, 500, 502, 503] {
            assert!(
                !is_key_scoped_status(status),
                "{status} is not the key's fault; rotating would only add latency"
            );
        }
    }
}
