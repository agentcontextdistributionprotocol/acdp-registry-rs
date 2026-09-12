//! The cross-backend full-text contract: what `q=` means on every backend.
//!
//! SQLite (FTS5) and Postgres (`tsvector`) disagreed on `q=` because they apply
//! different language rules. Measured on both engines before this existed:
//!
//! | query | sqlite | pg |
//! |---|---|---|
//! | `q=running` against "run report" | 0 rows | 1 row |
//! | `q=the` against "the quarterly figures" | 1 row | 0 rows |
//!
//! **Postgres's semantics won.** It is the production backend and stemming is
//! better search behaviour, so degrading it to reach agreement would have been
//! a genuine product regression rather than a fix. SQLite was brought up to it
//! instead, and the cost lands on SQLite's FTS index — derived data rebuilt
//! from `contexts`, so the change is reversible and loses nothing.
//!
//! Two mechanisms had to be matched, in two different places:
//!
//! * **Stemming** — `acdp-registry-sqlite/migrations/013_fts5_porter.sql`
//!   switches `contexts_fts` to `tokenize = 'porter unicode61'`; FTS5 then
//!   applies it at both index and query time.
//! * **Stopwords** — porter does *not* drop them (measured), so they are dropped
//!   query-side using the list below.
//!
//! The list lives here, in the crate both backends depend on, rather than inside
//! one of them: it is a shared contract, and the Postgres suite must read it to
//! check it against Postgres itself.

/// Postgres's `english` stopword list, verbatim.
///
/// Copied from the server's own `tsearch_data/english.stop` (PostgreSQL 16,
/// 127 entries — the snowball English list). A *published, versioned* list, not
/// one invented here.
///
/// It is not trusted on that basis alone. The Postgres parity suite asserts
/// every entry is still a stopword according to Postgres itself
/// (`to_tsvector('english', w)` comes back empty), so a divergence fails a test
/// instead of quietly skewing search results. That is what turns a hand-copied
/// table into a verified one — the characteristic failure of such tables is
/// that nobody notices when they go stale.
///
/// The one direction the check cannot cover is Postgres *gaining* a stopword
/// this list lacks; their file cannot be enumerated from SQL. That residual risk
/// is why the parity suite pins mechanisms rather than claiming exhaustive
/// agreement across the language.
pub const PG_ENGLISH_STOPWORDS: &[&str] = &[
    "i",
    "me",
    "my",
    "myself",
    "we",
    "our",
    "ours",
    "ourselves",
    "you",
    "your",
    "yours",
    "yourself",
    "yourselves",
    "he",
    "him",
    "his",
    "himself",
    "she",
    "her",
    "hers",
    "herself",
    "it",
    "its",
    "itself",
    "they",
    "them",
    "their",
    "theirs",
    "themselves",
    "what",
    "which",
    "who",
    "whom",
    "this",
    "that",
    "these",
    "those",
    "am",
    "is",
    "are",
    "was",
    "were",
    "be",
    "been",
    "being",
    "have",
    "has",
    "had",
    "having",
    "do",
    "does",
    "did",
    "doing",
    "a",
    "an",
    "the",
    "and",
    "but",
    "if",
    "or",
    "because",
    "as",
    "until",
    "while",
    "of",
    "at",
    "by",
    "for",
    "with",
    "about",
    "against",
    "between",
    "into",
    "through",
    "during",
    "before",
    "after",
    "above",
    "below",
    "to",
    "from",
    "up",
    "down",
    "in",
    "out",
    "on",
    "off",
    "over",
    "under",
    "again",
    "further",
    "then",
    "once",
    "here",
    "there",
    "when",
    "where",
    "why",
    "how",
    "all",
    "any",
    "both",
    "each",
    "few",
    "more",
    "most",
    "other",
    "some",
    "such",
    "no",
    "nor",
    "not",
    "only",
    "own",
    "same",
    "so",
    "than",
    "too",
    "very",
    "s",
    "t",
    "can",
    "will",
    "just",
    "don",
    "should",
    "now",
];

/// True when `token` is a Postgres `english` stopword.
///
/// Case-folded, because Postgres lowercases before consulting the list.
/// `to_lowercase` rather than `to_ascii_lowercase` so a non-ASCII token is not
/// silently treated as a different word.
pub fn is_pg_english_stopword(token: &str) -> bool {
    let lower = token.to_lowercase();
    PG_ENGLISH_STOPWORDS.contains(&lower.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_is_postgres_sized() {
        // Pins the count so an edit that drops or duplicates entries is caught
        // without needing a database.
        assert_eq!(
            PG_ENGLISH_STOPWORDS.len(),
            127,
            "PostgreSQL 16's english.stop has exactly 127 entries"
        );
    }

    #[test]
    fn membership_is_case_insensitive() {
        assert!(is_pg_english_stopword("the"));
        assert!(is_pg_english_stopword("The"));
        assert!(is_pg_english_stopword("THE"));
        assert!(!is_pg_english_stopword("report"));
        assert!(!is_pg_english_stopword("running"));
    }

    #[test]
    fn every_entry_is_lowercase_and_unique() {
        let mut seen = std::collections::HashSet::new();
        for w in PG_ENGLISH_STOPWORDS {
            assert_eq!(*w, w.to_lowercase(), "entry {w:?} must be lowercase");
            assert!(seen.insert(*w), "duplicate entry {w:?}");
        }
    }
}
