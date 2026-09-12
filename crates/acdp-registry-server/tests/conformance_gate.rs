//! Guards the conformance suite against a vacuous pass: `tests/conformance.rs` is
//! `#![cfg(feature = "storage-sqlite")]`, so a job that forgets that feature would
//! compile the entire suite away and still report green.
// `cfg!(feature = "storage-sqlite")` is a compile-time constant for any
// single build (clippy checks this crate with its default features, which
// include `storage-sqlite`), but it varies across the different
// `--features`/`--no-default-features` invocations this test is meant to
// guard — that's the whole point of the assertion, so silence the
// constant-value lint rather than drop it.
#[allow(clippy::assertions_on_constants)]
#[test]
fn require_mode_implies_the_conformance_suite_is_compiled_in() {
    if std::env::var("ACDP_REQUIRE_CONFORMANCE").is_ok() {
        assert!(
            cfg!(feature = "storage-sqlite"),
            "ACDP_REQUIRE_CONFORMANCE is set but `storage-sqlite` is off, so \
             tests/conformance.rs compiled to nothing — this run proves nothing. \
             Run the conformance job with --features storage-sqlite,playground."
        );
    }
}

/// CHARTER Rule 48: a documentation artifact no command can check is a defect
/// even while it is currently correct.
///
/// `docs/HTTP-API.md`'s "Status / code table" is a hand-kept set, and a
/// hand-kept set has no signal for the member that was never added. Measured
/// before this test existed: SEVEN wire codes the implementation can emit
/// appeared nowhere in that document — `invalid_signature`,
/// `unsupported_algorithm`, `key_not_authorized`, `embedded_too_large`,
/// `invalid_cursor`, `cursor_expired`, `invalid_witness_cosignature`. Three
/// were hidden behind the placeholder rows `(signature)` and `(payload)`,
/// which named no code a client could match on; the rest were simply absent.
///
/// Correcting the table by hand would have left the same defect for the next
/// arm added to `error.rs`. So the set is DERIVED here instead: every
/// `=> "wire_code"` arm in `acdp-registry-types/src/error.rs` must appear
/// somewhere in `docs/HTTP-API.md`.
///
/// Deliberately a substring check, not a table parse. The point is that the
/// code is *reachable* from the document at all; pinning the table's exact
/// shape would make every formatting edit a test failure, which is how
/// guards get deleted.
#[test]
fn every_wire_code_the_code_emits_is_documented() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/<crate>/ is two levels below the workspace root")
        .to_path_buf();

    let error_rs = root.join("crates/acdp-registry-types/src/error.rs");
    let src = std::fs::read_to_string(&error_rs)
        .unwrap_or_else(|e| panic!("read {}: {e}", error_rs.display()));

    // Scan ONLY the two functions that produce wire codes, and take every
    // string literal inside them. Scanning `=> "..."` alone was the first
    // attempt and it was wrong: a long match pattern uses a block body, so
    // `SchemaViolation | InvalidBody | MissingField => { "schema_violation" }`
    // has no `=> "`. The guard below is what caught that.
    let mut codes: Vec<String> = Vec::new();
    for fn_name in ["fn wire_code", "fn acdp_wire_code"] {
        let start = src
            .find(fn_name)
            .unwrap_or_else(|| panic!("{fn_name} not found in {}", error_rs.display()));
        // Function bodies in this file end at a closing brace in column 0.
        let body_end = src[start..]
            .find("\n}\n")
            .map(|e| start + e)
            .unwrap_or(src.len());
        let body = &src[start..body_end];
        let mut rest = body;
        while let Some(q) = rest.find('"') {
            rest = &rest[q + 1..];
            let Some(end) = rest.find('"') else { break };
            let c = &rest[..end];
            rest = &rest[end + 1..];
            if c.len() >= 4
                && c.chars().all(|ch| ch.is_ascii_lowercase() || ch == '_')
                && !codes.iter().any(|e| e == c)
            {
                codes.push(c.to_string());
            }
        }
    }
    codes.sort();

    // Guard the GENERATOR, not just its output: a scanner that silently
    // matched nothing would make the assertion below vacuously true, which is
    // precisely the failure mode this test exists to remove. Pin a floor and
    // two members that must always be present.
    assert!(
        codes.len() >= 15,
        "wire-code extraction found only {} codes in {} — the scanner is \
         broken, so the documentation check below would pass vacuously: {codes:?}",
        codes.len(),
        error_rs.display()
    );
    for required in ["not_found", "schema_violation"] {
        assert!(
            codes.iter().any(|c| c == required),
            "wire-code extraction did not find `{required}`, which certainly \
             exists — the scanner is broken: {codes:?}"
        );
    }

    let doc_path = root.join("docs/HTTP-API.md");
    let doc = std::fs::read_to_string(&doc_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", doc_path.display()));

    let undocumented: Vec<&String> = codes.iter().filter(|c| !doc.contains(c.as_str())).collect();
    assert!(
        undocumented.is_empty(),
        "these wire codes can be emitted by acdp-registry-types/src/error.rs but \
         appear NOWHERE in docs/HTTP-API.md: {undocumented:?}\n\
         Add them to the \"Status / code table\" with the HTTP status from \
         `http_status()`. A client cannot handle a code it has never been told \
         about, and a hand-kept table has no signal for the arm that was never \
         added — which is why this check is derived rather than maintained."
    );
}
