//! RFC-ACDP-0018 §6.1 bucket filters: the storage side.
//!
//! A bucket of length `n` is the first `n` digits of lowercase
//! hex(SHA-256(UTF-8(value))), where the value is the `lineage_id` or
//! `agent_id` string exactly as served. The SDK owns that derivation
//! ([`acdp::types::change::bucket`]) and the value grammar
//! ([`WatchFilter::validate_buckets`]); this module adds only what a store
//! needs on top:
//!
//! - [`bucket_hash`]: the full 64-digit hash every feed row stores per
//!   watched member, so a bucket of any length is a prefix of it;
//! - [`Bucket::range`]: a prefix as a half-open `[lower, upper)` range over
//!   that column, which every SQL backend serves with an index range scan.
//!
//! Every row's hashes are computed in [`crate::changes`] from the strings it
//! serves, so no backend can derive them differently. The Postgres insert
//! trigger (migration 021) is the one other producer, for an older binary
//! during a rolling deploy; a parity test pins it to [`bucket_hash`].

use acdp::error::AcdpError;
use acdp::types::change::{bucket, WatchFilter, MAX_CHANGE_BUCKET_LENGTH};

/// Digits in a stored bucket hash: the whole SHA-256 digest, so the column
/// survives any future raise of the spec's 16-digit bucket maximum.
pub const BUCKET_HASH_LEN: usize = 64;

/// The stored bucket hash of `value`: its §6.1 bucket at full digest length.
pub fn bucket_hash(value: &str) -> String {
    bucket(value, BUCKET_HASH_LEN)
}

/// One validated `lineage_bucket` / `agent_bucket` value: 1 to
/// `max_change_bucket_length` lowercase hex digits (§6.1).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Bucket(String);

impl Bucket {
    /// Parse `raw` against an advertised `max_len`, which is capped at the
    /// schema maximum [`MAX_CHANGE_BUCKET_LENGTH`]. The grammar is the SDK's;
    /// failures are its [`AcdpError::SchemaViolation`].
    pub fn parse(raw: &str, max_len: u16) -> Result<Self, AcdpError> {
        WatchFilter::new()
            .lineage_bucket(raw)
            .validate_buckets(Some(max_len.min(MAX_CHANGE_BUCKET_LENGTH)))?;
        Ok(Self(raw.to_owned()))
    }

    /// The bucket's hex digits.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether the stored hash `hash` lies in this bucket.
    pub fn contains(&self, hash: &str) -> bool {
        hash.starts_with(&self.0)
    }

    /// The bucket as a half-open range over stored hashes: a hash `h` is in
    /// it exactly when `lower <= h` and, when there is an upper bound,
    /// `h < upper`, compared bytewise.
    ///
    /// `lower` is the prefix itself. `upper` is the prefix incremented as a
    /// hex number of the same width (`"1f"` → `"20"`), so both bounds stay
    /// inside `[0-9a-f]`; a naive last-character increment would produce
    /// `':'` after `'9'`. An all-`f` prefix has no upper bound: every hash
    /// at or above it starts with it.
    pub fn range(&self) -> (String, Option<String>) {
        let mut digits = self.0.as_bytes().to_vec();
        for i in (0..digits.len()).rev() {
            let next = match digits[i] {
                b'f' => {
                    digits[i] = b'0';
                    continue;
                }
                b'9' => b'a',
                d => d + 1,
            };
            digits[i] = next;
            let upper = String::from_utf8(digits).expect("hex digits are ASCII");
            return (self.0.clone(), Some(upper));
        }
        (self.0.clone(), None)
    }
}

impl std::fmt::Display for Bucket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// rec-010's derivation vectors (spec 3f50697), inline so a regression
    /// fails without a spec checkout: `(bucket_of, sha256_hex, not_bucket_4)`.
    const VECTORS: [(&str, &str, Option<&str>); 5] = [
        (
            "lin:sha256:a3cb6a87c1601b9f6dddd8d54709b01df3e05f1095b43e6d58e3a862bf9906fa",
            "1f54677c32bda0fce357ac477603794f48739d9e1590b5ad5fa7ee48801432de",
            Some("6baa"),
        ),
        (
            "lin:sha256:9eb5e0ee5d9a110edc81f7e1fdafca3e3f521650fb58b8bb09c53f29bec58592",
            "fdd0e7e87d1c2056989122d13b8031a5be52be71358f0f4e99539a24c07a65fc",
            None,
        ),
        (
            "did:key:z6MkiTBz1ymuepAQ4HEHYSF1H8quG5GLVVQR3djdX3mDooWp",
            "ad90ec18fd5e0735b1e5eeab571e8dc9f733dcd65276387844c8236fefb87977",
            None,
        ),
        (
            "did:web:agents.example.com:test-producer",
            "7a3e4116a94f4d654b8953a260b2203e2504014c4a70234198f69304dbd1d79e",
            None,
        ),
        (
            "did:web:registry.example.com%3A8443",
            "cf5a338210f6c09d7715ee5a9271c03304b35a995e74db80da7db719595c5bf0",
            Some("d1b2"),
        ),
    ];

    #[test]
    fn bucket_hash_reproduces_the_rec_010_vectors() {
        for (value, sha, not_bucket_4) in VECTORS {
            let h = bucket_hash(value);
            assert_eq!(h, sha, "{value}");
            assert_eq!(h.len(), BUCKET_HASH_LEN);
            // Every `expected` prefix (1, 4, 8) is the SDK's bucket, and a
            // prefix of the stored hash.
            for n in [1, 4, 8] {
                let b = bucket(value, n);
                assert_eq!(b, sha[..n], "{value} at {n}");
                assert!(Bucket::parse(&b, 8).unwrap().contains(&h), "{value} at {n}");
            }
            // The traps: hashing the bare digest, or the percent-decoded DID.
            if let Some(wrong) = not_bucket_4 {
                assert_ne!(&h[..4], wrong, "{value}");
                assert!(!Bucket::parse(wrong, 4).unwrap().contains(&h), "{value}");
            }
        }
        // And the traps really are what the fixture says they are.
        let bare = VECTORS[0].0.trim_start_matches("lin:sha256:");
        assert_eq!(&bucket_hash(bare)[..4], "6baa");
        assert_eq!(
            &bucket_hash("did:web:registry.example.com:8443")[..4],
            "d1b2"
        );
    }

    #[test]
    fn parse_takes_the_sdk_grammar_up_to_the_advertised_length() {
        for (raw, max) in [("1", 4), ("1f54", 4), ("0123456789abcdef", 16), ("a", 16)] {
            assert_eq!(Bucket::parse(raw, max).unwrap().as_str(), raw);
        }
        for (raw, max) in [
            ("", 16),
            ("A", 16),
            ("1F54", 16),
            ("g", 16),
            ("1f54a", 4),
            ("0123456789abcdef0", 16),
            ("é", 16),
            ("１", 16),
            (" 1", 16),
        ] {
            let err = Bucket::parse(raw, max).expect_err(raw);
            assert!(
                matches!(err, AcdpError::SchemaViolation(_)),
                "{raw}: {err:?}"
            );
        }
        // An advertised length above the schema maximum is capped at 16.
        assert!(Bucket::parse("0123456789abcdef0", 64).is_err());
        assert!(Bucket::parse("0123456789abcdef", 64).is_ok());
    }

    #[test]
    fn range_edges() {
        let r = |p: &str| Bucket::parse(p, 16).unwrap().range();
        let some = |l: &str, u: &str| (l.to_string(), Some(u.to_string()));
        assert_eq!(r("0"), some("0", "1"));
        assert_eq!(r("9"), some("9", "a"), "never ':' after '9'");
        assert_eq!(r("e"), some("e", "f"));
        assert_eq!(r("f"), ("f".to_string(), None));
        assert_eq!(r("1f"), some("1f", "20"));
        assert_eq!(r("9f"), some("9f", "a0"));
        assert_eq!(r("af"), some("af", "b0"));
        assert_eq!(r("0fff"), some("0fff", "1000"));
        assert_eq!(r("ffff"), ("ffff".to_string(), None));
        let all_f = "f".repeat(16);
        assert_eq!(r(&all_f), (all_f.clone(), None));
        assert_eq!(
            r("0000000000000000"),
            some("0000000000000000", "0000000000000001")
        );
        for p in ["0", "9", "1f", "9f9", "fffe", "0123456789abcdef"] {
            let (lo, hi) = r(p);
            let hi = hi.unwrap();
            assert_eq!(hi.len(), lo.len(), "{p}: same width");
            assert!(hi
                .bytes()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
            assert!(lo < hi, "{p}");
        }
    }

    /// A deterministic splitmix64, so the property test needs no new
    /// dependency and every run checks the same pairs.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        }
        fn hex(&mut self, n: usize) -> String {
            (0..n)
                .map(|_| char::from(b"0123456789abcdef"[(self.next() % 16) as usize]))
                .collect()
        }
        fn len(&mut self) -> usize {
            1 + (self.next() % 16) as usize
        }
    }

    fn in_range(h: &str, (lo, hi): &(String, Option<String>)) -> bool {
        h >= lo.as_str() && hi.as_ref().is_none_or(|hi| h < hi.as_str())
    }

    /// For 10 000 (hash, prefix) pairs, range containment is exactly
    /// `starts_with`. Half the pairs put the hash just ABOVE the prefix's
    /// range (the prefix with its last digit decremented, or a hash that
    /// starts with the range's upper bound), so dropping the upper bound
    /// from [`Bucket::range`] fails this test on every run.
    #[test]
    fn range_containment_equals_prefix_match() {
        let mut rng = Rng(0x0018_0006_0001);
        let (mut inside, mut above) = (0, 0);
        for i in 0..10_000 {
            let n = rng.len();
            let (hash, prefix) = match i % 4 {
                // The hash's own prefix.
                0 => {
                    let h = rng.hex(BUCKET_HASH_LEN);
                    let p = h[..n].to_string();
                    (h, p)
                }
                // An unrelated prefix.
                1 => (rng.hex(BUCKET_HASH_LEN), rng.hex(n)),
                // The prefix one below the hash's: the hash sorts above
                // the prefix's range, and only the upper bound says so.
                2 => {
                    let mut h = rng.hex(BUCKET_HASH_LEN).into_bytes();
                    if h[n - 1] == b'0' {
                        h[n - 1] = b'1';
                    }
                    let mut p = h[..n].to_vec();
                    p[n - 1] = match p[n - 1] {
                        b'a' => b'9',
                        d => d - 1,
                    };
                    (String::from_utf8(h).unwrap(), String::from_utf8(p).unwrap())
                }
                // A hash that starts with the range's (exclusive) end.
                _ => {
                    let mut p = rng.hex(n);
                    if p.bytes().all(|c| c == b'f') {
                        p.replace_range(n - 1.., "e");
                    }
                    let end = Bucket::parse(&p, 16).unwrap().range().1.unwrap();
                    let h = format!("{end}{}", rng.hex(BUCKET_HASH_LEN - n));
                    (h, p)
                }
            };
            let b = Bucket::parse(&prefix, 16).unwrap();
            let want = hash.starts_with(&prefix);
            assert_eq!(
                in_range(&hash, &b.range()),
                want,
                "hash {hash} prefix {prefix} range {:?}",
                b.range()
            );
            assert_eq!(b.contains(&hash), want);
            match i % 4 {
                0 => {
                    assert!(want);
                    inside += 1;
                }
                2 | 3 => {
                    assert!(!want && hash.as_str() > prefix.as_str());
                    above += 1;
                }
                _ => {}
            }
        }
        assert_eq!((inside, above), (2_500, 5_000));
    }
}
