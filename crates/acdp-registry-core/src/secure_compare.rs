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
pub(crate) fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    ct_fold(a.iter().zip(b.iter())) == 0
}

/// The XOR fold, taken over an arbitrary iterator of byte pairs rather than
/// over the two slices directly.
///
/// **The generic parameter is not abstraction for its own sake — it is what
/// makes the non-short-circuit property assertable.** The property is about
/// *work performed*, not about the value returned: a short-circuiting
/// comparison produces exactly the same `bool` for every input, so no
/// assertion on the result can distinguish one from the other. (The test that
/// used to claim this property asserted only results, and passed when `ct_eq`
/// was replaced with `a == b`.) Taking an iterator lets a test pass one that
/// counts how many pairs were consumed, which is the property itself rather
/// than a proxy for it — and it is deterministic, where a wall-clock timing
/// test on a shared machine is a flake that eventually gets deleted.
///
/// `ct_fold` is private and `ct_eq` is its only caller, so a rewrite of
/// `ct_eq` that stops folding makes this function dead code — which is what
/// binds the test below to the real comparison, rather than leaving it
/// asserting a function nothing calls.
///
/// Verified, not assumed: CI's `cargo clippy --locked --workspace
/// --all-targets -- -D warnings` exits **101** with `function ct_fold is never
/// used` when `ct_eq`'s body is replaced by `a == b`. `--all-targets` is doing
/// the work there — it compiles the lib target separately from the test
/// target, so the test module's use of `ct_fold` does not keep it alive in the
/// lib's own compilation. A test-only build would not catch this.
fn ct_fold<'a, I>(pairs: I) -> u8
where
    I: Iterator<Item = (&'a u8, &'a u8)>,
{
    let mut diff = 0u8;
    for (x, y) in pairs {
        diff |= x ^ y;
    }
    diff
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

    /// #168 — a mismatch in the FIRST byte and a mismatch in the LAST must do
    /// the same work. `==` on `&[u8]` is free to stop at the first differing
    /// byte; this must not.
    ///
    /// **These assertions are about the RESULT, and the result cannot show the
    /// property.** They are kept because they are worth having, but on their
    /// own they were vacuous: this test passed unchanged when `ct_eq`'s body
    /// was replaced with `a == b` — the short-circuiting comparison it exists
    /// to forbid. The claim that "every input reaches the fold" was stated in
    /// this comment and asserted nowhere. `ct_fold_consumes_every_pair` below
    /// is the assertion that actually distinguishes the two.
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

    /// H-P item 2 — non-short-circuiting, asserted as **work performed**.
    ///
    /// The property is "a wrong first byte costs what a wrong last byte costs".
    /// Every assertion on `ct_eq`'s return value is blind to it, because a
    /// short-circuiting comparison returns the same `bool`; measured, not
    /// assumed — replacing the fold with `a == b` left all three tests above
    /// green. Wall-clock timing would see it but flakily, and a flaky security
    /// test gets deleted rather than fixed.
    ///
    /// So this counts pairs consumed instead. The fold must run to the end of
    /// the input even when the very first pair already differs, which is
    /// exactly what a short-circuit would skip.
    #[test]
    fn ct_fold_consumes_every_pair() {
        let secret = b"abcdefghijklmnop";

        for (label, other) in [
            ("first byte differs", b"Xbcdefghijklmnop"),
            ("last byte differs", b"abcdefghijklmnoX"),
            ("identical", b"abcdefghijklmnop"),
        ] {
            let consumed = std::cell::Cell::new(0usize);
            let diff = ct_fold(
                secret
                    .iter()
                    .zip(other.iter())
                    .inspect(|_| consumed.set(consumed.get() + 1)),
            );
            assert_eq!(
                consumed.get(),
                secret.len(),
                "{label}: the fold consumed {} of {} byte pairs — it stopped \
                 early, so the time it takes reveals how much of the presented \
                 credential matched",
                consumed.get(),
                secret.len()
            );
            // And the fold still has to be CORRECT, so this is not satisfiable
            // by a loop that consumes everything and computes nothing.
            assert_eq!(
                diff == 0,
                secret == other,
                "{label}: the fold consumed every pair but reported the wrong \
                 answer"
            );
        }
    }
}
