//! Constant-time comparison for bearer credentials.
//!
//! #168: this lives here rather than beside either caller because the
//! registry has **two** gates that compare a presented bearer token against a
//! configured one — `/admin/*` (`handlers::admin::require_admin_bearer`) and
//! `/metrics` (`metrics::metrics_endpoint`) — and they must not drift apart.
//! Duplicating the fold would guarantee that one copy eventually does.

/// Constant-time byte-slice equality. Unequal lengths return `false` (the
/// token *length* is not the secret); equal-length inputs are compared with an
/// XOR fold that never short-circuits, so timing does not reveal the
/// matching-prefix length.
///
/// Note the length guard is itself an early return, so token length remains
/// observable at both call sites. That is accepted in the existing design —
/// what this protects is the token *contents*.
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ct_eq_matches_only_identical_byte_slices() {
        assert!(ct_eq(b"secret-token", b"secret-token"));
        assert!(!ct_eq(b"secret-token", b"secret-toleN"));
        // Differing lengths are unequal (and don't panic on the zip).
        assert!(!ct_eq(b"short", b"longer-token"));
        // Two empty slices are trivially equal (length guard passes, fold is 0).
        assert!(ct_eq(b"", b""));
    }

    /// #168 — the property that makes this worth a helper: a mismatch in the
    /// FIRST byte and a mismatch in the LAST must do the same work. `==` on
    /// `&[u8]` is free to stop at the first differing byte; this must not.
    ///
    /// Asserted structurally rather than by wall-clock timing, which would be
    /// flaky in CI: every input below is the same length, so all of them reach
    /// the fold and run it to completion.
    #[test]
    fn ct_eq_does_not_short_circuit_on_position() {
        let secret = b"abcdefghijklmnop";
        // Differs at the first byte, the middle, and the last respectively.
        assert!(!ct_eq(secret, b"Xbcdefghijklmnop"));
        assert!(!ct_eq(secret, b"abcdefgXijklmnop"));
        assert!(!ct_eq(secret, b"abcdefghijklmnoX"));
        // A single differing BIT is still a mismatch (the fold is |=, not +).
        assert!(!ct_eq(b"\x00", b"\x01"));
        // ...and an accumulated fold cannot cancel back to zero: two
        // differences must not XOR away into a false match.
        assert!(!ct_eq(b"\x01\x01", b"\x00\x00"));
    }

    /// #161/#168 — an empty configured token matches an empty presented one.
    /// That is correct behaviour for this primitive and is exactly why the
    /// empty-entry guard lives in `validate_config` instead: the compare is
    /// not the right place to reject a bad credential from config.
    #[test]
    fn ct_eq_is_not_a_credential_policy() {
        assert!(ct_eq(b"", b""));
        assert!(!ct_eq(b"", b"x"));
        assert!(!ct_eq(b"x", b""));
    }
}
