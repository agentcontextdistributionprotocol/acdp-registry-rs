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

/// Wire codes the two producing functions in `acdp-registry-types/src/error.rs`
/// can emit, extracted from TEXT so that the guard over them is falsifiable here.
///
/// Scans ONLY the two functions that produce wire codes, taking every string
/// literal inside them. Scanning `=> "..."` alone was the first attempt and it was
/// wrong: a long match pattern uses a block body, so
/// `SchemaViolation | InvalidBody | MissingField => { "schema_violation" }` has no
/// `=> "`. The count guard at the call site is what caught that.
fn wire_codes_in(src: &str, origin: &str) -> Vec<String> {
    let mut codes: Vec<String> = Vec::new();
    for fn_name in ["fn wire_code", "fn acdp_wire_code"] {
        let start = src
            .find(fn_name)
            .unwrap_or_else(|| panic!("{fn_name} not found in {origin}"));
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
    codes
}

/// Wire codes `acdp-registry-types/src/error.rs` emits today. A ratchet, not a
/// floor -- see the assertion that reads it for why an exact number is affordable
/// here, and for what the floor it replaced let through.
const EXPECTED_WIRE_CODES: usize = 24;

/// The exact wire-code count must FAIL when the scanner loses a code — otherwise it
/// is a number nobody has shown to do anything.
///
/// Falsified against SYNTHETIC source rather than the real `error.rs`, which is in
/// another crate and outside this unit's path grant. That is also why
/// `wire_codes_in` takes text: a guard you cannot falsify without editing someone
/// else's file is a guard that never gets falsified.
#[test]
fn the_wire_code_scanner_is_pinned_exactly_and_falsifiable() {
    const SRC: &str = r#"
fn wire_code(&self) -> &'static str {
    match self {
        Self::NotFound => "not_found",
        Self::Schema | Self::InvalidBody => { "schema_violation" }
        Self::Rate => "rate_limited",
    }
}
fn acdp_wire_code(err: &AcdpError) -> &'static str {
    match err {
        AcdpError::Sig => "invalid_signature",
    }
}
"#;
    let found = wire_codes_in(SRC, "<synthetic>");
    assert_eq!(
        found.len(),
        4,
        "the extractor must find all four codes, including the BLOCK-bodied arm \
         that has no `=> \"`: {found:?}"
    );

    // Lose one code, the way a refactor does. An exact count sees it; the floor this
    // replaced did not -- 3 of 4 satisfies any threshold the full set satisfies,
    // which is the entire defect this unit is about.
    let one_gone = SRC.replace("        Self::Rate => \"rate_limited\",\n", "");
    let fewer = wire_codes_in(&one_gone, "<synthetic>");
    assert_eq!(
        fewer.len(),
        3,
        "removing one arm must change the extracted count: {fewer:?}"
    );
    assert!(
        !fewer.contains(&"rate_limited".to_string()),
        "and the code lost must be the one removed: {fewer:?}"
    );
    assert_ne!(
        fewer.len(),
        found.len(),
        "so an exact assertion against the full count goes RED on this input, \
         which is precisely what `codes.len() >= 15` did not do"
    );
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
    let mut codes = wire_codes_in(&src, &error_rs.display().to_string());
    codes.sort();

    // Guard the GENERATOR, not just its output: a scanner that silently matched
    // nothing would make the assertion below vacuously true, which is precisely the
    // failure mode this test exists to remove.
    //
    // WAS `codes.len() >= 15` against 24 actual -- it tolerated the scanner losing
    // NINE codes, and each lost code is one whose documentation silently stops being
    // checked. No independent derivation of this set exists at the string level:
    // `http_status()` matches on enum variants, and several variants share a single
    // code (`SchemaViolation | InvalidBody | MissingField => "schema_violation"`),
    // so variants cannot be counted against codes. The exact count is therefore a
    // deliberate ratchet, and not an extra tax: adding a wire code ALREADY requires
    // an edit to docs/HTTP-API.md, and this test is the thing that enforces it.
    // `wire_codes_in` is a pure function over text precisely so this number can be
    // falsified without editing another crate's source.
    assert_eq!(
        codes.len(),
        EXPECTED_WIRE_CODES,
        "wire-code extraction found {} codes in {}, expected exactly \
         {EXPECTED_WIRE_CODES}. If you ADDED a wire code: update this constant and \
         add the code to the status table in docs/HTTP-API.md -- enforcing that \
         pairing is what this test is for. If you did not, the scanner is broken and \
         the documentation check below would pass for every code it can no longer \
         see: {codes:?}",
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

/// CHARTER Rule 48 again, for the other hand-kept set in the docs: the route
/// tables. `README.md` listed `/healthz` as "Storage liveness" and carried no
/// `/livez` row at all after #239 split the two — a route can be mounted and
/// documented nowhere, and a prose table has no signal for the row nobody added.
///
/// Every `.route("...")` path mounted in `acdp-registry-core/src/lib.rs` must
/// appear in `docs/HTTP-API.md` (the reference) — and the handful a new operator
/// meets first must also appear in `README.md`.
///
/// TWO STATED LIMITS, because a guard whose reach is undocumented gets trusted
/// past it:
///
/// 1. The doc check is `contains`, so a nested path documented alone satisfies
///    its parent (`/contexts/{ctx_id}/body` in the docs also satisfies
///    `/contexts/{ctx_id}`). Acceptable — documenting a child while omitting the
///    parent is not the drift that has occurred here — but it is not exact.
///
/// 2. DIRECTIONALITY, the same caveat lane-1 recorded for
///    `every_route_in_the_core_router_is_classified`: this fails on a route that
///    EXISTS but is undocumented. It does NOT fail on a documented route that
///    has been deleted from the router — for that, a wire test that actually
///    calls the endpoint is the only real guard. Two different failure modes;
///    this one covers the direction that has actually bitten twice.
#[test]
fn every_mounted_route_is_documented() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/<crate>/ is two levels below the workspace root")
        .to_path_buf();

    let lib_rs = root.join("crates/acdp-registry-core/src/lib.rs");
    let src = std::fs::read_to_string(&lib_rs)
        .unwrap_or_else(|e| panic!("read {}: {e}", lib_rs.display()));

    // Scan `.route(` and then skip whitespace to the opening quote — rustfmt
    // splits long calls across lines, so `.route("` as a literal misses them.
    // That is not hypothetical: `/.well-known/did.json` is mounted in exactly
    // that wrapped form and the first version of this scanner never saw it,
    // while a `>= 15` floor happily passed at 19. A floor cannot catch
    // under-counting by one, so the mechanism guard below is an EQUALITY
    // against the number of `.route(` calls in the file.
    // Strip comments first: this file's own doc prose mentions `.route(...)`,
    // and counting that as a mount makes the equality below unsatisfiable.
    // A `//` inside a string literal is not a comment, hence the quote parity.
    let src: String = src
        .lines()
        .map(|line| match line.find("//") {
            Some(i) if line[..i].matches('"').count() % 2 == 0 => &line[..i],
            _ => line,
        })
        .collect::<Vec<_>>()
        .join("\n");
    let src = src.as_str();

    let calls = src.matches(".route(").count();
    let mut found: Vec<String> = Vec::new();
    for (i, _) in src.match_indices(".route(") {
        let rest = &src[i + 7..];
        let Some(q) = rest.find('"') else { continue };
        // Only whitespace may sit between `(` and the path literal; anything
        // else means this is a `.route(` we do not understand, and the
        // equality assertion below will catch it rather than silently skip it.
        if !rest[..q].chars().all(char::is_whitespace) {
            continue;
        }
        let after = &rest[q + 1..];
        let Some(end) = after.find('"') else { continue };
        found.push(after[..end].to_string());
    }

    assert_eq!(
        found.len(),
        calls,
        "route extraction parsed {} of the {calls} `.route(` calls in {} — the \
         scanner is silently skipping mounts, so the checks below under-cover \
         by exactly the routes it missed. Parsed: {found:?}",
        found.len(),
        lib_rs.display()
    );

    let mut routes: Vec<String> = Vec::new();
    for r in &found {
        if r.starts_with('/') && !routes.iter().any(|e| e == r) {
            routes.push(r.clone());
        }
    }
    routes.sort();
    assert_eq!(
        routes.len(),
        found.len(),
        "extracted a non-path or a duplicate route literal; parsed {found:?}"
    );

    // Named members, because an equality alone still passes if BOTH the router
    // and the scanner lose a route together.
    for required in ["/healthz", "/livez", "/contexts", "/.well-known/did.json"] {
        assert!(
            routes.iter().any(|r| r == required),
            "route extraction did not find `{required}`, which is certainly \
             mounted — the scanner is broken: {routes:?}"
        );
    }

    // `{ctx_id}`-style params are spelled the same way in the docs, so a plain
    // substring check is enough and stays robust to table formatting.
    let api = std::fs::read_to_string(root.join("docs/HTTP-API.md")).expect("read HTTP-API.md");
    let undocumented: Vec<&String> = routes
        .iter()
        .filter(|r| !api.contains(r.as_str()))
        .collect();
    assert!(
        undocumented.is_empty(),
        "these routes are mounted in acdp-registry-core/src/lib.rs but appear \
         nowhere in docs/HTTP-API.md: {undocumented:?}"
    );

    // README is the operator's first contact: the unauthenticated probes and the
    // core publish/retrieve surface must be discoverable there too.
    let readme = std::fs::read_to_string(root.join("README.md")).expect("read README.md");
    let readme_must_list = [
        "/livez",
        "/healthz",
        "/contexts/search",
        "/.well-known/acdp.json",
    ];
    let missing: Vec<&str> = readme_must_list
        .iter()
        .copied()
        .filter(|r| !readme.contains(r))
        .collect();
    assert!(
        missing.is_empty(),
        "README.md's endpoint table is missing {missing:?}. These are the routes an \
         operator meets first — /livez in particular, because documenting /healthz \
         as \"liveness\" is what wires a storage-gated probe to a k8s livenessProbe \
         and restart-loops healthy pods during a database outage."
    );
}

/// `docs/MULTI-TENANCY.md` tells clients the search refill loop is capped at a
/// specific number of inner pages. That number is a promise about paging
/// behaviour — a client that trusts a stale one under-drains its results — and
/// until this test it was a hand-typed literal in prose with nothing tying it to
/// `SEARCH_REFILL_MAX_PAGES`.
///
/// Rule 48: ship the number in a form a command regenerates. The constant is
/// private to `acdp-registry-core`, so this reads it from source rather than
/// importing it; the scan is guarded so a rename fails loudly instead of
/// silently skipping the comparison.
#[test]
fn documented_search_refill_cap_matches_the_constant() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/<crate>/ is two levels below the workspace root")
        .to_path_buf();

    let src_path = root.join("crates/acdp-registry-core/src/handlers/context.rs");
    let src = std::fs::read_to_string(&src_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", src_path.display()));

    const DECL: &str = "const SEARCH_REFILL_MAX_PAGES: usize = ";
    let i = src.find(DECL).unwrap_or_else(|| {
        panic!(
            "SEARCH_REFILL_MAX_PAGES is no longer declared in {} — this test can \
             no longer check the documented cap, and docs/MULTI-TENANCY.md still \
             states a number. Repoint the scan or drop the claim.",
            src_path.display()
        )
    });
    let rest = &src[i + DECL.len()..];
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    let cap: usize = digits.parse().unwrap_or_else(|e| {
        panic!("could not parse SEARCH_REFILL_MAX_PAGES value from {digits:?}: {e}")
    });
    // NOT a floor standing in for a count: `cap` is one parsed configuration value
    // and `> 0` is its actual semantic requirement -- zero disables the refill loop.
    // The exact value is pinned against the document two assertions below, which is
    // where an equality belongs. Left deliberately unchanged by U-538.
    assert!(cap > 0, "a zero refill cap would disable the loop entirely");

    let doc_path = root.join("docs/MULTI-TENANCY.md");
    let doc = std::fs::read_to_string(&doc_path).expect("read MULTI-TENANCY.md");

    // Guard the anchor as well as the number: if the prose is rewritten so this
    // sentence disappears, the `contains` below would pass vacuously forever.
    assert!(
        doc.contains("SEARCH_REFILL_MAX_PAGES"),
        "docs/MULTI-TENANCY.md no longer names SEARCH_REFILL_MAX_PAGES, so this \
         test is checking nothing. Restore the reference or delete this test."
    );
    assert!(
        doc.contains(&format!("**{cap}** today")),
        "docs/MULTI-TENANCY.md does not state the current refill cap of {cap} \
         (expected the literal \"**{cap}** today\"). A client that trusts a stale \
         cap stops paging early and silently under-drains its results."
    );
}

/// Walk `crates/**/*.rs` and concatenate. Used as a presence corpus, not for
/// parsing — a symbol that appears nowhere in any Rust source is certainly a
/// stale citation; one that appears somewhere might still be cited in the wrong
/// file, which this does not claim to catch.
/// Drop `#[cfg(test)]`-gated items from a Rust source string, or return
/// `None` if that cannot be done confidently.
///
/// Needed because "shipped `src/` tree" and "shipped code" are not the same
/// set: a `#[cfg(test)] mod tests` block lives inside `src/`, so any scan of
/// `src/` that treats what it finds as production surface reports test-only
/// constructs as operator-facing. That is a false-positive class, and a guard
/// that cries wolf gets routed around rather than fixed.
///
/// Brace-matched rather than line-based. For each attribute, whichever of `{`
/// or `;` comes first decides: a braced item drops through its matching `}`,
/// an unbraced one (`#[cfg(test)] use …;`) drops through the `;`.
///
/// **The `None` is the whole design.** This counts braces without tracking
/// string literals, so a test containing `record("not json {", …)` — which
/// `acdp-registry-store/src/log.rs` does today — never balances. The tempting
/// fix is a smarter parser. The important fix is choosing which way to fail:
/// returning a truncated string would silently delete production code from the
/// corpus and make a documentation gate under-report, which is indistinguishable
/// from "everything is documented". Returning `None` instead tells the caller to
/// scan that file UNSTRIPPED, so the worst case is the original false positive —
/// loud, and a human resolves it — never a silent miss.
fn strip_cfg_test(src: &str) -> Option<String> {
    const ATTR: &str = "#[cfg(test)]";
    let mut out = String::with_capacity(src.len());
    let mut rest = src;
    while let Some(at) = rest.find(ATTR) {
        out.push_str(&rest[..at]);
        let after = &rest[at + ATTR.len()..];
        let brace = after.find('{');
        let semi = after.find(';');
        match (brace, semi) {
            // An item with a body: skip through the matching close brace.
            (Some(open), semi) if semi.is_none_or(|s| open < s) => {
                let bytes = after.as_bytes();
                let mut depth = 0usize;
                let mut end = None;
                for (i, b) in bytes.iter().enumerate().skip(open) {
                    match b {
                        b'{' => depth += 1,
                        b'}' => {
                            depth -= 1;
                            if depth == 0 {
                                end = Some(i + 1);
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                // Unbalanced (a brace inside a literal). Refuse rather than
                // truncate — see the doc comment.
                rest = &after[end?..];
            }
            // `#[cfg(test)] use …;` — no body, ends at the semicolon.
            (_, Some(s)) => rest = &after[s + 1..],
            // A guard does not count toward exhaustiveness, so this arm also
            // catches `(Some(_), None)` when the guard above declines it.
            _ => return None,
        }
    }
    out.push_str(rest);
    Some(out)
}

/// Every `.rs` file under `dir`, as separate strings.
///
/// Separate matters: `strip_cfg_test` must run per file. Stripping one
/// concatenated blob lets an unbalanced block in one file swallow the files
/// after it — which is exactly how a production `ACDP_LOG_FORMAT` read in
/// `acdp-registry-server/src/main.rs` went missing while this guard was
/// being written, caught only because the caller pins the variable set by
/// equality rather than by a lower bound.
fn rust_source_file_texts(dir: &std::path::Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == "target") {
                continue;
            }
            rust_source_file_texts(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            if let Ok(s) = std::fs::read_to_string(&path) {
                out.push(s);
            }
        }
    }
}

fn rust_source_corpus(dir: &std::path::Path, out: &mut String) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == "target") {
                continue;
            }
            rust_source_corpus(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            if let Ok(s) = std::fs::read_to_string(&path) {
                out.push_str(&s);
                out.push('\n');
            }
        }
    }
}

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// `docs/AUTHENTICATION.md` is the densest source-citing document in the repo:
/// it explains why three bearer parsers disagree, and every claim is anchored to
/// a specific function. It used to anchor them with LINE NUMBERS — thirteen of
/// them — which is the most rot-prone citation form there is, because nothing
/// reports a pin that has slipped.
///
/// Two had already slipped when this test was written:
/// `context.rs:1350-1367` for `caller_from_headers` (really at 1362, pointing
/// instead at the tail of an unrelated handler) and `lib.rs:154-156` for the
/// `/metrics` mount (really ~110 lines away — that range is the `auth`
/// subrouter). Both read as precise and were wrong, which is worse than vague.
///
/// So the pins were replaced with symbol names, and this test enforces the new
/// form. Rule 48: the fact is now shipped in a shape a command can check.
///
/// LIMIT, stated: presence is checked against the whole-workspace corpus, so
/// this catches a symbol that no longer exists ANYWHERE — a rename or a
/// deletion. It does not catch a symbol that still exists but has moved to a
/// different file than the one cited.
#[test]
fn authentication_doc_cites_symbols_that_exist_and_never_line_numbers() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/<crate>/ is two levels below the workspace root")
        .to_path_buf();
    let doc_path = root.join("docs/AUTHENTICATION.md");
    let doc = std::fs::read_to_string(&doc_path).expect("read AUTHENTICATION.md");

    // Collect every backtick-delimited span once; all three checks read from it.
    let mut spans: Vec<&str> = Vec::new();
    let mut rest = doc.as_str();
    while let Some(open) = rest.find('`') {
        let after = &rest[open + 1..];
        match after.find('`') {
            Some(close) => {
                spans.push(&after[..close]);
                rest = &after[close + 1..];
            }
            None => break,
        }
    }
    // WAS `spans.len() > 100` against 263 actual: the tokenizer could drop 163 of
    // its spans and still pass, while every check below silently stopped covering
    // them. Replaced by an EQUALITY that does not rot as the document is edited --
    // the loop above consumes exactly two backticks per span, so the span count
    // must equal half the backtick characters in the file. Counting characters is a
    // different operation from scanning for pairs, so a loop that breaks early
    // (its `None => break`), or skips a span, diverges from it immediately. This is
    // the guard the floor was pretending to be, and unlike an exact count of spans
    // it needs no edit when someone adds a sentence.
    let backticks = doc.matches('`').count();
    assert_eq!(
        backticks % 2,
        0,
        "{} contains an ODD number of backtick characters ({backticks}), so at \
         least one inline span is unterminated. The scanner below silently drops \
         the tail, and every check that reads its output would then cover less \
         than the document says.",
        doc_path.display()
    );
    assert_eq!(
        spans.len(),
        backticks / 2,
        "the span scanner found {} spans in {} but the file holds {backticks} \
         backtick characters, i.e. {} pairs. The scanner is dropping spans, and \
         every check below would then pass for the spans it never saw.",
        spans.len(),
        doc_path.display(),
        backticks / 2
    );

    // 1. No line-number pins, in any form, anywhere in the document. This is the
    //    rule that prevents the drift from coming back; the two checks after it
    //    only validate what replaced them.
    let pins: Vec<&&str> = spans
        .iter()
        .filter(|s| {
            s.split(".rs:")
                .skip(1)
                .any(|tail| tail.starts_with(|c: char| c.is_ascii_digit()))
        })
        .collect();
    assert!(
        pins.is_empty(),
        "docs/AUTHENTICATION.md cites source by line number: {pins:?}. Line pins \
         rot silently — two of the original thirteen were already pointing at \
         unrelated code. Cite the function or test by name instead; this test \
         then checks that the name still exists."
    );

    // 2. Every cited source file exists.
    let cited_paths: Vec<&&str> = spans
        .iter()
        .filter(|s| s.starts_with("crates/") && s.ends_with(".rs"))
        .collect();
    // DELIBERATELY A LOWER BOUND, and what it does not catch is stated rather than
    // left to be discovered. The exact number (10 today) is a property of the
    // DOCUMENT, not an invariant of the code: pinning it would redden this gate on
    // any edit that cites one more file, and a guard that fails on correct input is
    // one someone deletes rather than fixes. So this cannot detect the filter
    // silently matching 8 of 10 citations.
    //
    // What made the floor dangerous was that it was ALSO standing in as the
    // tokenizer's vacuity guard, and that job has moved: `spans.len()` is now
    // pinned exactly against the file's backtick count above, so a broken scanner
    // fails there, by name, instead of being tolerated here. This floor now guards
    // only the one thing left -- the `crates/`-prefix filter matching nothing at
    // all -- which is why a coarse threshold is adequate for it.
    assert!(
        cited_paths.len() >= 8,
        "only {} crate source paths cited — expected at least 8. The span scanner \
         is pinned exactly above, so this is the `crates/…rs` FILTER, or the \
         document genuinely stopped citing source: {cited_paths:?}",
        cited_paths.len()
    );
    let gone: Vec<&&&str> = cited_paths
        .iter()
        .filter(|p| !root.join(p).exists())
        .collect();
    assert!(
        gone.is_empty(),
        "docs/AUTHENTICATION.md cites source files that do not exist: {gone:?}"
    );

    // 3. Every backticked snake_case identifier still exists in the workspace.
    let mut corpus = String::new();
    rust_source_corpus(&root.join("crates"), &mut corpus);
    // A GENUINE lower bound, deliberately kept, and what it misses is stated rather
    // than left to be found: a byte total has no exact expected value that would not
    // rot on literally every commit. It therefore cannot detect the walk dropping a
    // whole crate -- the remaining seven still exceed 100KB. What it does catch is
    // the walk returning nothing or nearly nothing, which is the failure that would
    // make check 3 below report every identifier as missing (or pass vacuously).
    // The file-level completeness of a walk like this IS pinned exactly, by count,
    // in `every_directly_read_env_var_is_documented`.
    assert!(
        corpus.len() > 100_000,
        "source corpus is only {} bytes — the walk is broken and check 3 would \
         report every identifier as missing (or, worse, pass vacuously if the \
         document were empty)",
        corpus.len()
    );

    let mut idents: Vec<&str> = Vec::new();
    for s in &spans {
        let ok = s.len() >= 4
            && s.starts_with(|c: char| c.is_ascii_lowercase() || c == '_')
            && s.chars().all(is_ident_char);
        if ok && !idents.contains(s) {
            idents.push(s);
        }
    }
    // Also deliberately a lower bound, for the same reason and with the same
    // stated blind spot: the exact identifier count is a property of the prose. It
    // cannot detect the ident filter dropping some of them. As above, the
    // tokenizer's own completeness is pinned exactly earlier in this test, so what
    // remains here is the filter, and the two named members below (`required`) pin
    // specific results rather than a quantity.
    assert!(
        idents.len() >= 20,
        "only {} backticked identifiers extracted — expected at least 20. The span \
         scanner is pinned exactly above, so suspect the identifier filter: \
         {idents:?}",
        idents.len()
    );
    for required in [
        "caller_from_headers",
        "require_admin_bearer",
        "extract_bearer",
    ] {
        assert!(
            idents.contains(&required),
            "`{required}` is no longer cited in AUTHENTICATION.md — either the \
             document stopped explaining the parser it names, or the extraction \
             is broken: {idents:?}"
        );
    }

    let stale: Vec<&&str> = idents
        .iter()
        .filter(|id| {
            !corpus.match_indices(*id).any(|(i, _)| {
                let before = corpus[..i].chars().next_back();
                let after = corpus[i + id.len()..].chars().next();
                !before.is_some_and(is_ident_char) && !after.is_some_and(is_ident_char)
            })
        })
        .collect();
    assert!(
        stale.is_empty(),
        "docs/AUTHENTICATION.md cites identifiers that exist nowhere in \
         crates/**/*.rs: {stale:?}. A renamed or deleted function leaves the \
         prose around it describing behaviour nothing implements."
    );
}

/// #220: the root `CHANGELOG.md` grew to thousands of lines under a single
/// `## [Unreleased]` heading while `0.1.0`, `0.1.1` and `0.1.2` had all shipped.
/// It was the only artefact in the repo asserting that its own released content
/// was unreleased — the eight per-crate changelogs `release-plz` maintains were
/// correct throughout. Decision 15 in `DECISIONS.md` records why it was retired
/// to a pointer rather than split by version (the `0.1.1` content is interleaved
/// into `0.1.0` entries, including an amendment written inside a `0.1.0`
/// paragraph, so "every line lands in exactly one section" is unsatisfiable).
///
/// This guards the outcome in both directions: the root file must not reacquire
/// a version heading, and the records it delegates to must still be there.
#[test]
fn root_changelog_stays_a_pointer() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/<crate>/ is two levels below the workspace root")
        .to_path_buf();

    let changelog = std::fs::read_to_string(root.join("CHANGELOG.md")).expect("read CHANGELOG.md");

    // 1. No version sections. `## [Unreleased]` is the exact shape that was
    //    retired; a `## [0.1.3]` would be the same mistake made deliberately.
    let version_headings: Vec<&str> = changelog
        .lines()
        .filter(|l| l.starts_with("## [") || l.starts_with("## v"))
        .collect();
    assert!(
        version_headings.is_empty(),
        "CHANGELOG.md has reacquired version sections: {version_headings:?}. This \
         file is a pointer — releases are per-crate and release-plz owns those \
         files. Narrative entries belong in docs/ENGINEERING-LOG.md. See decision \
         15 in DECISIONS.md."
    );
    assert!(
        changelog.lines().count() < 100,
        "CHANGELOG.md is {} lines. It is a pointer; it grew back.",
        changelog.lines().count()
    );

    // 2. It must actually point somewhere. A pointer that has lost its targets
    //    is worse than the file it replaced: no release notes and no narrative.
    for target in [
        "docs/ENGINEERING-LOG.md",
        "DECISIONS.md",
        "crates/<crate>/CHANGELOG.md",
    ] {
        assert!(
            changelog.contains(target),
            "CHANGELOG.md no longer directs readers to `{target}`"
        );
    }
    assert!(
        root.join("docs/ENGINEERING-LOG.md").exists(),
        "docs/ENGINEERING-LOG.md is missing but CHANGELOG.md points at it — the \
         narrative history has been lost, not relocated"
    );

    // 3. The delegated-to records must cover the version actually being built.
    //    This is the release-truth check the retired file was failing: it is not
    //    enough that per-crate changelogs exist, they have to be current.
    let manifest =
        std::fs::read_to_string(root.join("Cargo.toml")).expect("read workspace Cargo.toml");
    let version = manifest
        .lines()
        .find_map(|l| {
            let l = l.trim_start();
            l.strip_prefix("version")?
                .trim_start()
                .strip_prefix('=')?
                .trim()
                .strip_prefix('"')?
                .split('"')
                .next()
        })
        .expect("workspace [workspace.package] version");
    assert!(
        version.split('.').count() == 3,
        "parsed a workspace version of {version:?}, which is not a semver triple \
         — the parse is wrong and the check below would be meaningless"
    );

    let heading = format!("## [{version}]");
    let mut checked = 0usize;
    let mut stale: Vec<String> = Vec::new();
    for entry in std::fs::read_dir(root.join("crates"))
        .expect("read crates/")
        .flatten()
    {
        let path = entry.path().join("CHANGELOG.md");
        if !path.exists() {
            continue;
        }
        checked += 1;
        let text = std::fs::read_to_string(&path).expect("read per-crate CHANGELOG.md");
        if !text.contains(&heading) {
            stale.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    // WAS `checked >= 8` against exactly 8 crates -- tight today, and blind in the
    // direction that actually happens: a NINTH crate arrives, has no CHANGELOG.md,
    // the `continue` above skips it, `checked` stays 8, and the floor is satisfied
    // while that crate's release notes go unchecked forever. The expectation is
    // therefore derived from the workspace's own declaration of what exists.
    let members = workspace_member_crates(&root);
    let mut without_changelog: Vec<&String> = members
        .iter()
        .filter(|m| !root.join("crates").join(m).join("CHANGELOG.md").exists())
        .collect();
    without_changelog.sort();
    assert!(
        without_changelog.is_empty(),
        "these workspace members have no CHANGELOG.md, so the staleness check \
         below cannot see them at all: {without_changelog:?}. release-plz writes \
         one per released crate; a member without one is either unreleased (say so \
         here) or was skipped."
    );
    assert_eq!(
        checked,
        members.len(),
        "the crates/ walk read {checked} per-crate changelogs but the workspace \
         declares {} members — the walk and Cargo.toml disagree about which crates \
         exist, so the staleness check below covers an unknown subset",
        members.len()
    );
    stale.sort();
    assert!(
        stale.is_empty(),
        "the workspace is at {version} but these crates' changelogs have no \
         `{heading}` section: {stale:?}. Release notes are delegated to these \
         files, so a gap here means the release is undocumented everywhere."
    );
}

// ---------------------------------------------------------------------------
// U-538: retiring the floor-style guards.
//
// Several checks in this file guarded a scanner with `assert!(found.len() >= N)`.
// A FLOOR CANNOT CATCH UNDERCOUNTING -- it is satisfied by the very walk that is
// silently missing items, which is the failure it was written to detect. Measured
// on this tree at the time of the change: `docs/*.md` was 10 against a floor of 8,
// `crates/*/src/**.rs` was 40 against a floor of 20, and the per-crate changelog
// walk was 8 against a floor of 8 -- tight today, and blind in the direction that
// actually happens, a NINTH crate arriving with no changelog.
//
// What catches undercounting is a SECOND enumeration derived a different way,
// asserted EQUAL. `git ls-files` and a filesystem walk share no code, so a walk
// that swallows an error through `.flatten()` diverges from it; the root
// `Cargo.toml`'s `members` list is a declarative third view of the same set.
// Equality also catches the opposite direction a floor can never see: a file that
// exists and is untracked, or is tracked and missing from disk.
// ---------------------------------------------------------------------------

/// Every path git tracks under `root`, as `/`-joined strings.
///
/// `-z` so paths containing spaces or newlines survive intact, and a failure to
/// run git is FATAL rather than falling back to a glob -- a fallback scope is how
/// a sweep silently narrows, and this helper exists to widen one.
fn git_tracked_paths(root: &std::path::Path) -> Vec<String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "-z"])
        .output()
        .unwrap_or_else(|e| {
            panic!(
                "could not run `git ls-files` in {}: {e}. These guards derive a                  SECOND enumeration from git on purpose and do not fall back to a                  glob: a single enumeration cannot detect its own omissions.",
                root.display()
            )
        });
    assert!(
        out.status.success(),
        "`git ls-files` failed in {}: {}",
        root.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout)
        .split('\0')
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .collect()
}

/// Discrepancies between two independent enumerations of what should be one set.
/// Empty means they agree. Both directions are reported, because they are
/// different bugs: only-in-`walked` is an untracked file, only-in-`reference` is a
/// walk that dropped something.
///
/// `what_walked`/`what_reference` name the two methods in the output, so a failure
/// says which enumeration to go and fix rather than just that they differ.
fn enumeration_disagreements(
    walked: &[String],
    reference: &[String],
    what_walked: &str,
    what_reference: &str,
) -> Vec<String> {
    let mut out = Vec::new();
    for w in walked {
        if !reference.contains(w) {
            out.push(format!(
                "{w}: found by {what_walked}, absent from {what_reference}"
            ));
        }
    }
    for r in reference {
        if !walked.contains(r) {
            out.push(format!(
                "{r}: found by {what_reference}, absent from {what_walked}"
            ));
        }
    }
    out.sort();
    out
}

/// The workspace's crates, from the root `Cargo.toml`'s `members` array.
///
/// A DECLARATIVE enumeration: it is what cargo itself builds, so it cannot drift
/// from the workspace the way a directory walk can drift from either.
fn workspace_member_crates(root: &std::path::Path) -> Vec<String> {
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).expect("read Cargo.toml");
    let start = manifest.find("members = [").expect(
        "root Cargo.toml has a `members = [` array; the parse below is worthless without it",
    );
    let rest = &manifest[start..];
    let end = rest
        .find(']')
        .expect("unterminated `members` array in root Cargo.toml");
    let members: Vec<String> = rest[..end]
        .lines()
        .filter_map(|l| l.trim().strip_prefix('"'))
        .filter_map(|l| l.split('"').next())
        .filter(|l| l.starts_with("crates/"))
        .map(|l| l.trim_start_matches("crates/").to_string())
        .collect();
    assert!(
        !members.is_empty(),
        "parsed 0 crates from the root Cargo.toml `members` array -- the parse is          broken, and every check deriving its expectation from it would pass          vacuously, which is the exact defect this helper was written to remove"
    );
    members
}

/// The two-enumeration helper must FAIL on exactly the input a floor tolerates —
/// otherwise this unit replaced one unfalsified assertion with another.
///
/// The decisive assertion here is not that the helper reports a disagreement; it is
/// that **the floor it replaced does not**. Both are checked against the same input,
/// in the same test, so the improvement is a property of the code rather than a
/// claim in a commit message.
#[test]
fn the_two_enumeration_guard_catches_what_a_floor_tolerates() {
    let ten: Vec<String> = (0..10).map(|i| format!("doc-{i}.md")).collect();

    // Identical enumerations: silent.
    assert!(
        enumeration_disagreements(&ten, &ten, "walk", "git").is_empty(),
        "two identical enumerations must not disagree"
    );

    // A walk that dropped ONE of ten -- the failure a scanner actually has.
    let nine: Vec<String> = ten.iter().skip(1).cloned().collect();
    let missed = enumeration_disagreements(&nine, &ten, "walk", "git");
    assert_eq!(
        missed.len(),
        1,
        "a walk missing one of ten must report exactly one disagreement: {missed:?}"
    );
    assert!(
        missed[0].contains("doc-0.md") && missed[0].contains("absent from walk"),
        "the disagreement must NAME the dropped item and say which method missed \
         it, or a reader cannot tell which enumeration to fix: {missed:?}"
    );

    // THE POINT OF THE UNIT: the floor this replaced is satisfied by that same
    // input. `9 >= 8` is true, so the old guard passed while a document went
    // unchecked. A floor cannot catch undercounting because the count it accepts
    // is the count the broken walk produces.
    assert!(
        nine.len() >= 8,
        "sanity: the replaced floor really was satisfied by the 9-of-10 input, \
         which is what made it useless"
    );

    // The opposite direction, which a floor can NEVER see at any threshold: an
    // extra item on disk that the reference does not know about. `11 >= 8` holds.
    let mut eleven = ten.clone();
    eleven.push("untracked.md".to_string());
    let extra = enumeration_disagreements(&eleven, &ten, "walk", "git");
    assert_eq!(
        extra.len(),
        1,
        "an untracked extra must be reported too: {extra:?}"
    );
    assert!(
        extra[0].contains("untracked.md") && extra[0].contains("absent from git"),
        "and it must be reported as the OTHER direction -- an untracked file is a \
         different bug from a dropped one: {extra:?}"
    );

    // Both empty is agreement between two broken methods, which is why every
    // caller asserts non-emptiness separately rather than trusting agreement.
    assert!(
        enumeration_disagreements(&[], &[], "walk", "git").is_empty(),
        "two empty enumerations agree -- the callers must check emptiness \
         themselves, and this asserts that the helper alone does NOT catch it"
    );
}

/// `workspace_member_crates` must parse the real manifest, and must refuse rather
/// than return an empty set — an empty expectation makes every check derived from
/// it pass vacuously, which is the defect this whole unit is about.
#[test]
fn workspace_members_parse_to_the_real_crate_set() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/<crate>/ is two levels below the workspace root")
        .to_path_buf();
    let members = workspace_member_crates(&root);

    // A second, independent derivation of the same set: the directories on disk.
    // Asserted EQUAL, not "at least", for the reason this unit exists.
    let mut dirs: Vec<String> = std::fs::read_dir(root.join("crates"))
        .expect("read crates/")
        .flatten()
        .filter(|e| e.path().join("Cargo.toml").exists())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    dirs.sort();
    let mut sorted = members.clone();
    sorted.sort();
    let disagreements =
        enumeration_disagreements(&dirs, &sorted, "the crates/ walk", "Cargo.toml members");
    assert!(
        disagreements.is_empty(),
        "the crates/ directory and the workspace `members` array disagree about \
         which crates exist:\n  {}",
        disagreements.join("\n  ")
    );
    assert!(
        members.contains(&"acdp-registry-server".to_string()),
        "the parse must find acdp-registry-server, which certainly is a member: \
         {members:?}"
    );
}

/// `docs/README.md` carries a "Map" table naming every document under `docs/`.
/// Hand-kept set, no signal for the row nobody added — the same shape as the
/// README route table and the wire-code list. `ENGINEERING-LOG.md` arrived via
/// #220 and the table did not notice.
///
/// Directional, like its siblings: this catches a document that exists and is
/// unlisted. A listed document that has been deleted is caught instead by the
/// link being dead, which is not something this test checks.
#[test]
fn every_docs_page_is_listed_in_the_docs_index() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/<crate>/ is two levels below the workspace root")
        .to_path_buf();
    let docs = root.join("docs");
    let index = std::fs::read_to_string(docs.join("README.md")).expect("read docs/README.md");

    let mut pages: Vec<String> = std::fs::read_dir(&docs)
        .expect("read docs/")
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            (name.ends_with(".md") && name != "README.md").then_some(name)
        })
        .collect();
    pages.sort();

    // WAS `pages.len() >= 8`, a floor over a set of 10 -- so the walk could drop
    // two documents and the index check below would silently stop covering them.
    // A floor is satisfied by the omission it is meant to detect. git is a second
    // enumeration sharing no code with `read_dir`, and equality also catches the
    // direction a floor never could: a document on disk that nobody tracked.
    let tracked_pages: Vec<String> = git_tracked_paths(&root)
        .into_iter()
        .filter_map(|p| p.strip_prefix("docs/").map(str::to_string))
        .filter(|p| p.ends_with(".md") && p != "README.md" && !p.contains('/'))
        .collect();
    let disagreements = enumeration_disagreements(
        &pages,
        &tracked_pages,
        "the docs/ directory walk",
        "git ls-files",
    );
    assert!(
        disagreements.is_empty(),
        "the two enumerations of docs/*.md disagree, so one of them is missing \
         documents and the index check below would not cover them:\n  {}",
        disagreements.join("\n  ")
    );
    assert!(
        !pages.is_empty(),
        "both enumerations of docs/*.md are EMPTY, which they agree on and which \
         is still wrong: two broken methods agree. docs/ has had pages since #220."
    );

    let unlisted: Vec<&String> = pages.iter().filter(|p| !index.contains(*p)).collect();
    assert!(
        unlisted.is_empty(),
        "these documents exist under docs/ but are absent from the Map table in \
         docs/README.md, so nothing links to them: {unlisted:?}"
    );
}

/// Environment variables the binary reads **directly** — `std::env::var("…")`
/// rather than through the `ACDP_REGISTRY_<SECTION>__<FIELD>` layer — are
/// invisible to anyone reading the config reference, because they correspond to
/// no TOML key that the reference documents.
///
/// Two of them were the only way to configure `auth.tenant_agents` and
/// `playground.pinned_keys` on a deployment with no config file (Railway
/// "deploy from image"), and they appeared in no document at all. A third,
/// `ACDP_LOG_FORMAT`, was mentioned only in the narrative engineering log.
///
/// Rule 48: derive the set from the source instead of maintaining it by hand,
/// because a hand-maintained list cannot catch the variable nobody added to it.
#[test]
fn every_directly_read_env_var_is_documented() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/<crate>/ is two levels below the workspace root")
        .to_path_buf();

    // Only shipped `src/` trees — `tests/` carries its own env knobs
    // (`ACDP_REQUIRE_PG`, `ACDP_SPEC_DIR`, …) which are CI controls, not
    // operator configuration, and have no place in the config reference.
    //
    // But excluding `tests/` is NOT sufficient, which is what this guard got
    // wrong first time round: a `#[cfg(test)] mod tests` block lives inside
    // `src/`, so a test-only env read there was reported as undocumented
    // operator config, complete with a message telling the author an operator
    // could not discover it. `strip_cfg_test` removes those regions below.
    let mut files: Vec<String> = Vec::new();
    for crate_dir in std::fs::read_dir(root.join("crates"))
        .expect("read crates/")
        .flatten()
    {
        let src = crate_dir.path().join("src");
        if src.is_dir() {
            rust_source_file_texts(&src, &mut files);
        }
    }
    // WAS `files.len() > 20` over a set of 40: the walk could drop HALF the
    // workspace's source and still pass, which makes the env-var sweep below
    // report "everything documented" while never reading 20 files. The walk
    // collects file TEXTS, so it is counted against git rather than compared
    // path-by-path -- the count equality is what a floor was standing in for.
    let tracked_src = git_tracked_paths(&root)
        .into_iter()
        .filter(|p| p.starts_with("crates/") && p.ends_with(".rs") && p.contains("/src/"))
        .count();
    assert_eq!(
        files.len(),
        tracked_src,
        "the crates/*/src walk found {} .rs files and git tracks {} -- one of the \
         two enumerations is missing files, and the sweep below would then report \
         no findings for the files it never read",
        files.len(),
        tracked_src
    );
    assert!(
        tracked_src > 0,
        "git tracks ZERO .rs files under crates/*/src, which the walk can agree \
         with while both are broken"
    );
    let sources: String = files.concat();
    // A GENUINE lower bound, kept alongside the exact file-count equality above.
    // No exact byte total exists that survives the next commit, so this cannot see
    // the concatenation losing a large file's CONTENTS while the file count stays
    // right -- a failure the count equality above also cannot see. That gap is real
    // and unguarded; it is written down rather than implied.
    assert!(
        sources.len() > 100_000,
        "src corpus is only {} bytes — the walk is broken",
        sources.len()
    );

    // Falsify `strip_cfg_test` on a synthetic input rather than trusting that
    // it works because the repo happens to contain no offender today. The
    // load-bearing case is the THIRD assertion: an over-eager stripper that
    // swallowed everything after the first `#[cfg(test)]` would satisfy the
    // first two and silently hide every production read below it.
    {
        let sample = concat!(
            "fn prod() { std::env::var(\"ACDP_KEEP_ME\"); }\n",
            "#[cfg(test)]\n",
            "mod tests {\n",
            "    fn t() { std::env::var(\"ACDP_DROP_ME\"); }\n",
            "    fn nested() { if true { let _ = 1; } }\n",
            "}\n",
            "fn after() { std::env::var(\"ACDP_KEEP_ME_TOO\"); }\n",
        );
        let stripped = strip_cfg_test(sample).expect("the sample is balanced");
        assert!(
            !stripped.contains("ACDP_DROP_ME"),
            "strip_cfg_test left a #[cfg(test)] body in place: {stripped}"
        );
        assert!(
            stripped.contains("ACDP_KEEP_ME"),
            "strip_cfg_test dropped production code BEFORE the test block: {stripped}"
        );
        assert!(
            stripped.contains("ACDP_KEEP_ME_TOO"),
            "strip_cfg_test dropped production code AFTER the test block — it \
             over-stripped, which hides real operator config: {stripped}"
        );

        // And the refusal path: a brace inside a string literal must yield
        // `None`, not a truncated string. This is the case that actually
        // occurs (acdp-registry-store/src/log.rs) and the one whose wrong
        // answer is silent.
        let unbalanced = concat!(
            "fn prod() { std::env::var(\"ACDP_KEEP_ME\"); }\n",
            "#[cfg(test)]\n",
            "mod tests {\n",
            "    fn t() { let _ = record(\"not json {\"); }\n",
            "}\n",
        );
        assert!(
            strip_cfg_test(unbalanced).is_none(),
            "a brace inside a string literal must make the stripper REFUSE, so \
             the caller scans the file whole; returning a truncated string \
             would delete production code from the corpus silently"
        );
    }

    // Strip PER FILE, never the concatenated blob: an unbalanced block in one
    // file must not be able to swallow the next one. A file the stripper
    // refuses is scanned whole, which can only produce a loud false positive.
    let raw_len = sources.len();
    let mut refused = 0usize;
    let sources: String = files
        .iter()
        .map(|f| match strip_cfg_test(f) {
            Some(stripped) => stripped,
            None => {
                refused += 1;
                f.clone()
            }
        })
        .collect();
    assert!(
        sources.len() < raw_len,
        "stripping removed nothing from a {raw_len}-byte corpus, so either no \
         `#[cfg(test)]` block exists under crates/*/src (one does) or the \
         stripper never ran; {refused} file(s) were refused"
    );
    assert!(
        refused < files.len() / 4,
        "the stripper refused {refused} of {} files — that is too many to be \
         the known literal-brace cases, and every refused file is scanned as \
         though its tests were production code",
        files.len()
    );

    // `env::var("ACDP…")` / `env::var_os("ACDP…")`, however the path is spelled.
    let mut vars: Vec<String> = Vec::new();
    for pat in ["env::var(\"ACDP", "env::var_os(\"ACDP"] {
        let open = pat.rfind('"').expect("pattern contains the opening quote");
        for (i, _) in sources.match_indices(pat) {
            let after = &sources[i + open + 1..];
            if let Some(end) = after.find('"') {
                let name = after[..end].to_string();
                if !vars.contains(&name) {
                    vars.push(name);
                }
            }
        }
    }
    vars.sort();

    // An EQUALITY, not a `>= n` floor. A floor and the defect point the same
    // way: a scanner that silently stops finding things produces FEWER items,
    // which a lower bound accepts as long as some survive, so the guard is
    // blind to precisely the failure it was written for. This population is
    // small and changes rarely, so pin it exactly.
    //
    // Adding a directly-read `ACDP_*` env var is MEANT to fail here. That is
    // the gate: add the name below and document it in docs/CONFIGURATION.md.
    let expected = [
        "ACDP_LOG_FORMAT",
        "ACDP_REGISTRY_AUTH__TENANT_AGENTS_JSON",
        "ACDP_REGISTRY_CONFIG",
        "ACDP_REGISTRY_PLAYGROUND__PINNED_KEYS_JSON",
    ];
    assert_eq!(
        vars, expected,
        "the set of directly-read ACDP env vars under crates/*/src changed.\n\
         found:    {vars:?}\n\
         expected: {expected:?}\n\
         If you ADDED one: list it above and document it in \
         docs/CONFIGURATION.md — that pairing is the whole point of this test. \
         If one DISAPPEARED and you did not remove it, the scan or \
         `strip_cfg_test` is broken, which is the case a `>= n` floor here \
         used to let through."
    );

    let doc = std::fs::read_to_string(root.join("docs/CONFIGURATION.md"))
        .expect("read docs/CONFIGURATION.md");
    // Limit, stated rather than implied: this is substring containment, so a
    // var whose name EMBEDS a documented one satisfies it (documenting
    // `ACDP_LOG_FORMAT` would cover a hypothetical `ACDP_LOG_FORMAT_EXTRA`).
    // Found while falsifying this assertion — renaming the doc mention to
    // `ACDP_LOG_FORMAT_RENAMED_AWAY` left the check green, because the probe
    // still contained the original. The set equality above is what bounds the
    // population, so the containment check only has to answer "is this name
    // written down somewhere", and a new name cannot arrive unnoticed.
    let undocumented: Vec<&String> = vars.iter().filter(|v| !doc.contains(v.as_str())).collect();
    assert!(
        undocumented.is_empty(),
        "these environment variables are read directly by the binary but appear \
         nowhere in docs/CONFIGURATION.md: {undocumented:?}. They have no TOML \
         key, so the Reference section does not cover them and an operator has \
         no way to discover them short of reading the source."
    );
}

/// H-J: no tracked file may contain a merge-conflict marker.
///
/// This family has produced five incidents. The most recent pair is the reason
/// the scope below is "every tracked file" and not a list: #238 swept
/// `ASSUMPTIONS.md`, left two diff3 base markers behind in `CHANGELOG.md`, and
/// reported success — the sweep's scope was one file, and nothing could tell it
/// that the mechanism reached further. Those two survived a `git mv` into
/// `docs/ENGINEERING-LOG.md` before being caught by hand.
///
/// Three markers, and the third is the one that actually bit us twice: seven
/// `<`, seven `>`, and seven `|` — the diff3 base marker, which ordinary
/// conflict resolution rarely produces and which is therefore the one a
/// hand-written check omits.
///
/// **The marker bytes are built at runtime, never written as literals.** If this
/// file contained the marker text it would match itself, and the fix for that
/// would be excepting this path — the hand-maintained allowlist this guard
/// exists to eliminate. Constructing them means no path needs an exception, so
/// the guard genuinely has none.
///
/// Scans **bytes**, not UTF-8: the repository has no binary tracked files today,
/// and a guard that starts erroring the day someone adds a PNG is a guard that
/// gets weakened rather than fixed.
#[test]
fn no_tracked_file_contains_a_conflict_marker() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/<crate>/ is two levels below the workspace root")
        .to_path_buf();

    // Derive the file set from git, never from a glob someone maintains
    // (Rule 48). `-z` so paths with spaces or newlines survive intact.
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["ls-files", "-z"])
        .output()
        .unwrap_or_else(|e| {
            panic!(
                "could not run `git ls-files` in {}: {e}. This guard derives its \
                 scope from git on purpose; it does not fall back to a glob, \
                 because a fallback scope is how the last sweep missed a file.",
                root.display()
            )
        });
    assert!(
        out.status.success(),
        "`git ls-files` failed in {}: {}",
        root.display(),
        String::from_utf8_lossy(&out.stderr)
    );

    let listed: Vec<&[u8]> = out
        .stdout
        .split(|&b| b == 0)
        .filter(|p| !p.is_empty())
        .collect();

    // Named members rather than a count floor (Rule 55/64): a floor cannot
    // detect an enumeration that silently came back short, which is exactly what
    // a broken `ls-files` invocation produces.
    let listed_paths: Vec<String> = listed
        .iter()
        .map(|p| String::from_utf8_lossy(p).into_owned())
        .collect();
    for required in ["Cargo.toml", "README.md", "docs/ENGINEERING-LOG.md"] {
        assert!(
            listed_paths.iter().any(|p| p == required),
            "`git ls-files` did not list `{required}`, which is certainly \
             tracked — the enumeration is broken, so every check below would \
             pass vacuously ({} paths listed)",
            listed_paths.len()
        );
    }

    // Seven of the byte, at line start, followed by a space or end-of-line.
    // Git emits exactly seven; requiring the boundary keeps a markdown
    // blockquote or table row from reading as a marker.
    const RUN: usize = 7;
    // One of each, so this literal cannot match itself (see the doc comment).
    let marker_bytes = *b"<>|";

    let mut findings: Vec<String> = Vec::new();
    let mut missing: Vec<String> = Vec::new();
    let mut scanned = 0usize;

    for path in &listed_paths {
        let full = root.join(path);
        let Ok(bytes) = std::fs::read(&full) else {
            // A listed-but-unreadable path is reported, never silently skipped:
            // silent skipping is the failure mode this whole unit is about.
            missing.push(path.clone());
            continue;
        };
        scanned += 1;
        for (i, line) in bytes.split(|&b| b == b'\n').enumerate() {
            for &m in &marker_bytes {
                let run = line.iter().take_while(|&&b| b == m).count();
                let bounded = match line.get(RUN) {
                    None => true,
                    Some(&b) => b == b' ' || b == b'\r',
                };
                if run == RUN && bounded {
                    let marker = String::from_utf8(vec![m; RUN]).expect("ascii");
                    findings.push(format!("{path}:{}: {marker}", i + 1));
                }
            }
        }
    }

    // Equality, not a floor: proves the loop visited every path git listed.
    assert_eq!(
        scanned + missing.len(),
        listed_paths.len(),
        "scanned {scanned} + {} unreadable != {} listed — the loop skipped \
         paths, so a marker could sit in one of them and this test would still \
         pass",
        missing.len(),
        listed_paths.len()
    );
    assert!(
        missing.is_empty(),
        "these paths are tracked but could not be read, so they went unscanned: \
         {missing:?}"
    );

    // Enumerate the offenders; a count would say a marker exists without saying
    // where, and the finding IS the set.
    assert!(
        findings.is_empty(),
        "merge-conflict markers found in tracked files:\n  {}\n\nThese reach \
         `main` as ordinary-looking content — a conflicted region resolved by \
         hand leaves the base marker behind most often, and nothing else in the \
         build notices. Delete the marker lines and check the surrounding \
         region kept the right side of the conflict.",
        findings.join("\n  ")
    );
}

// ---------------------------------------------------------------------------
// U-504 AC-8, re-pointed by U-536: the couplings around the spec pin.
//
// Until U-536 the pin lived in `.github/workflows/ci.yml` and `mutants.yml`
// DERIVED it, by grepping the 40-hex `ref:` out of that file. This test guarded
// that derivation. The derivation is gone: `.spec-pin` is now the single
// declarative source and BOTH workflows read it through
// `.github/actions/read-spec-pin`, so the invariants change shape -- from "the
// grep still finds it" to "nobody restates the pin, and everybody reads the file".
//
// What has NOT changed is why this is a test and not a comment. **Rule 48,
// exactly: a doc artifact no command can check is a defect while it is still
// correct.** Four couplings here are invisible from any single file:
//
//   * `mutants.yml` and `ci.yml` must resolve the SAME spec revision. They are
//     two files with no reference to each other; only the pin file joins them.
//   * `bump-spec.yml` hands acdp-ci's reusable bumper exactly ONE filename. A
//     pin restated anywhere else is never bumped -- it goes stale silently and
//     the two jobs start measuring different spec versions.
//   * That bumper is a bot IN ANOTHER REPOSITORY, so `.spec-pin`'s line order and
//     anchor count are an external contract a comment cannot reach.
//   * `tests/conformance.rs` refuses a spec tree that is not at the pin, reading
//     the same file.
//
// The verification gap is what makes it worth a test. `mutants.yml` is
// `schedule:` + `workflow_dispatch` with NO `pull_request` trigger -- deliberately,
// a 23-minute mutation run has no business gating a PR -- so a PR that breaks its
// wiring cannot turn it red. The breakage would surface on the following Monday's
// cron, detached from the change that caused it, in a job whose failure reads as
// "the ratchet is broken" rather than "someone edited a different file". This test
// moves the signal back to the PR that causes it.
//
// Written as a pure function over the four files' TEXT rather than as assertions
// against the real paths, for two reasons. It is falsifiable -- every invariant is
// shown to fail against a synthetic restructuring below, which is the whole point
// -- and a synthetic input can be made to break in one specific way, which the
// real file cannot without breaking CI for everyone.
// ---------------------------------------------------------------------------

/// A 40-hex run ANYWHERE in the line. This is what acdp-ci's bumper matches, so
/// it is what the line-order invariant has to reason about: a 64-hex digest
/// contains a 40-hex run, and the bumper would happily return its first 40
/// characters as the current pin.
fn has_hex40_run(line: &str) -> bool {
    let b = line.as_bytes();
    let is_hex = |c: u8| c.is_ascii_digit() || (b'a'..=b'f').contains(&c);
    let mut run = 0usize;
    for &c in b {
        if is_hex(c) {
            run += 1;
            if run >= 40 {
                return true;
            }
        } else {
            run = 0;
        }
    }
    false
}

/// Lines in `text` that DECLARE `key` with a value satisfying `pred`, 1-indexed.
///
/// A whole-line declaration at column 0, not an occurrence of a value anywhere.
/// The distinction is load-bearing rather than pedantic: the pinned sha also
/// appears in `docs/ENGINEERING-LOG.md` as narrative history, so any "appears
/// exactly once in the repository" check over a value is a false positive waiting
/// for someone to write a sentence. What every consumer parses -- the shell in
/// `read-spec-pin`, the bumper's awk, `conformance.rs` -- is the LINE SHAPE.
fn pin_declarations(text: &str, key: &str, pred: impl Fn(&str) -> bool) -> Vec<usize> {
    text.lines()
        .enumerate()
        .filter(|(_, l)| {
            l.strip_prefix(key)
                .and_then(|r| r.strip_prefix(": "))
                .map(&pred)
                .unwrap_or(false)
        })
        .map(|(i, _)| i + 1)
        .collect()
}

fn is_sha40(v: &str) -> bool {
    v.len() == 40
        && v.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

fn is_owner_repo(v: &str) -> bool {
    let ok = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
    };
    let mut parts = v.split('/');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(o), Some(r), None) => ok(o) && ok(r),
        _ => false,
    }
}

/// `uses:` LINES, not mentions, whose value contains `needle`.
///
/// The line anchor is not decoration. `ci.yml` discusses `checkout-spec@` in
/// three comments around the step itself; a substring search over the file
/// matches those and reports 4 usages where there is 1. That is not hypothetical
/// -- it is the bug the U-502 extraction shipped with.
fn uses_lines(text: &str, needle: &str) -> Vec<usize> {
    text.lines()
        .enumerate()
        .filter(|(_, l)| {
            let t = l.trim_start().trim_start_matches("- ");
            t.starts_with("uses:") && t.contains(needle)
        })
        .map(|(i, _)| i + 1)
        .collect()
}

/// A `ref:` line carrying a 40-hex literal, at any indentation. An action `uses:`
/// pin is also a 40-hex sha and must NOT count: a guard that banned every 40-hex
/// string would fail against the correct file, which is how a guard gets deleted.
fn literal_ref_lines(text: &str) -> Vec<String> {
    text.lines()
        .enumerate()
        .filter(|(_, l)| {
            l.trim_start()
                .strip_prefix("ref:")
                .map(|r| is_sha40(r.trim()))
                .unwrap_or(false)
        })
        .map(|(i, l)| format!("line {}: {}", i + 1, l.trim()))
        .collect()
}

/// Every invariant the `.spec-pin` wiring depends on. One string per violation;
/// empty means the single-source-of-truth shape is intact.
fn spec_pin_violations(
    spec_pin: &str,
    ci_yml: &str,
    mutants_yml: &str,
    bump_yml: &str,
) -> Vec<String> {
    let mut out = Vec::new();

    // (1) Exactly one `ref:` declaration. Zero means no consumer can find the
    //     pin; two means they need not all choose the same value, and two jobs
    //     resolving different revisions is the exact failure this file exists to
    //     prevent.
    let refs = pin_declarations(spec_pin, "ref", is_sha40);
    if refs.len() != 1 {
        out.push(format!(
            "invariant 1: .spec-pin has {} `ref: <40 hex>` declarations {:?}, expected \
             exactly 1. Zero and two are different bugs with the same cure: one line.",
            refs.len(),
            refs
        ));
    }

    // (2) Exactly one `repository:` declaration -- and this one is an EXTERNAL
    //     contract. acdp-ci's bumper locates the pin by finding a line naming the
    //     spec repository and REFUSES TO BUMP AT ALL when it counts more than one,
    //     rather than rewrite one and leave the rest stale. A second such line
    //     therefore does not corrupt the pin; it silently stops the pin ever
    //     moving again, which is worse because nothing goes red.
    let repos = pin_declarations(spec_pin, "repository", is_owner_repo);
    if repos.len() != 1 {
        out.push(format!(
            "invariant 2: .spec-pin has {} `repository: <owner>/<name>` declarations \
             {:?}, expected exactly 1. acdp-ci's bump-spec-ref counts these as pin \
             anchors and declines to bump a file with two, so the pin would freeze \
             silently rather than fail loudly.",
            repos.len(),
            repos
        ));
    }

    // (3) `ref:` must be the FIRST line carrying a 40-hex run. The bumper takes
    //     the first 40-hex value on a `ref:`-ish line below its anchor, and
    //     `conformance-digest:`'s 64 hex characters CONTAIN a 40-hex run -- so
    //     with the two lines swapped it reads the digest's first 40 characters as
    //     the current pin. Falsified against the bumper's own awk, not theorised.
    let first_hex40 = spec_pin.lines().position(has_hex40_run).map(|i| i + 1);
    match (first_hex40, refs.first()) {
        (Some(first), Some(&r)) if first != r => out.push(format!(
            "invariant 3: .spec-pin's first 40-hex run is on line {first}, but the \
             `ref:` declaration is on line {r}. The bumper reads the first such value \
             below its anchor; a 64-hex digest above `ref:` is a 40-hex run, so it \
             would adopt the digest's first 40 characters as the pin."
        )),
        _ => {}
    }

    // (4) EXACTLY ONE BUMPER ANCHOR, counted THE WAY THE BUMPER COUNTS -- which is
    //     not the way invariant 2 counts, and that difference is why both exist.
    //     acdp-ci's bump-spec-ref runs, over every line of the file:
    //
    //       index($0, "repository: " SPEC) || index($0, "acdp-ci/actions/checkout-spec@")
    //
    //     A SUBSTRING search, anywhere in the line, COMMENTS INCLUDED. So a comment
    //     that SPELLS either anchor form becomes a second anchor, and the bumper then
    //     refuses to bump the file at all rather than rewrite one and leave the rest
    //     stale: the pin freezes silently instead of failing loudly. That is why
    //     `.spec-pin`'s comments describe the two forms instead of quoting them, and
    //     this is the assertion that keeps that true -- a prose rule about prose,
    //     which is exactly the kind nothing else in the build can check.
    //
    //     Invariant 2's column-0 declaration count CANNOT see a comment. Found by
    //     falsification: the spelled-anchor case below was written against invariant
    //     2, and invariant 2 stayed silent -- while citing this external contract in
    //     its own failure message.
    if let Some(&r) = repos.first() {
        let spec = spec_pin
            .lines()
            .nth(r - 1)
            .and_then(|l| l.strip_prefix("repository: "))
            .map(str::trim)
            .unwrap_or_default()
            .to_string();
        let needle = format!("repository: {spec}");
        let anchors: Vec<usize> = spec_pin
            .lines()
            .enumerate()
            .filter(|(_, l)| l.contains(&needle) || l.contains("acdp-ci/actions/checkout-spec@"))
            .map(|(i, _)| i + 1)
            .collect();
        if anchors.len() != 1 {
            out.push(format!(
                "invariant 4: .spec-pin has {} lines matching the BUMPER's anchor \
                 patterns {:?}, expected exactly 1. It counts by substring over every \
                 line -- `repository: {spec}` or `acdp-ci/actions/checkout-spec@`, \
                 comments included -- and declines to bump a file with two anchors, so \
                 the pin would stop moving with nothing going red. Describe an anchor \
                 form in prose; never spell it.",
                anchors.len(),
                anchors
            ));
        }
    }

    // (5)-(9) apply to both workflows. The pair is the point: they are two files
    // with no reference to each other that must resolve the SAME revision.
    for (name, text) in [("ci.yml", ci_yml), ("mutants.yml", mutants_yml)] {
        // (5) The pin is not restated. This is the repair that DEFEATS the whole
        //     design: pasting a literal makes a wiring error go away locally and
        //     decouples that job's spec from every other consumer. `bump-spec.yml`
        //     passes the bumper exactly one filename, so a pasted copy is never
        //     rewritten -- it goes stale in silence.
        let pasted = literal_ref_lines(text);
        if !pasted.is_empty() {
            out.push(format!(
                "invariant 5: {name} restates the spec pin literally instead of reading \
                 .spec-pin: {pasted:?}. Only ONE file is bumped, so a second copy goes \
                 stale silently and this job starts measuring a different spec revision \
                 from the others. It must stay an expression."
            ));
        }

        // (6) It reads the pin through the shared action -- exactly once. Two
        //     reader steps would mean two `id:`s and no way for this test to know
        //     which output the checkout consumes.
        let readers = uses_lines(text, "./.github/actions/read-spec-pin");
        if readers.len() != 1 {
            out.push(format!(
                "invariant 6: {name} has {} `uses: ./.github/actions/read-spec-pin` \
                 lines {:?}, expected exactly 1. Zero means this job no longer reads the \
                 single source and its spec revision came from somewhere unaudited.",
                readers.len(),
                readers
            ));
        }

        // (7) Exactly one spec checkout, so "the pin" is unambiguous in this file.
        let checkouts = uses_lines(text, "checkout-spec@");
        if checkouts.len() != 1 {
            out.push(format!(
                "invariant 7: {name} has {} `uses: …checkout-spec@` lines {:?}, expected \
                 exactly 1. A second spec checkout can be wired to a different ref, which \
                 is the ambiguity the single source removes.",
                checkouts.len(),
                checkouts
            ));
        }

        // (8) The reader must come BEFORE the checkout that consumes its outputs.
        //     A step cannot reference a later step's outputs: the expression
        //     resolves to the empty string and `checkout-spec` silently takes its
        //     own default branch -- a green job measuring the wrong tree. That is
        //     precisely the class U-536 exists to close, so it gets an assertion
        //     rather than a convention.
        if let (Some(&reader), Some(&checkout)) = (readers.first(), checkouts.first()) {
            if reader > checkout {
                out.push(format!(
                    "invariant 8: {name} reads the pin at line {reader}, BELOW the spec \
                     checkout at line {checkout}. A step cannot consume a later step's \
                     outputs -- the expression resolves to empty and the checkout falls \
                     back to its default branch, green and wrong."
                ));
            }
        }

        // (9) `repository:` is passed through from the pin rather than left to
        //     `checkout-spec`'s own default, which it currently matches. If the two
        //     ever diverged, the bumper would resolve a sha from the repository
        //     `.spec-pin` names while this job checked out a different one -- a
        //     silent wrong-tree pass, the failure mode this unit exists to close,
        //     reappearing inside the fix for it.
        let passthrough = text
            .lines()
            .filter(|l| {
                let t = l.trim_start();
                t.starts_with("repository:") && t.contains(".outputs.repository")
            })
            .count();
        if passthrough != 1 {
            out.push(format!(
                "invariant 9: {name} has {passthrough} `repository: <…outputs.repository>` \
                 lines, expected exactly 1. Relying on checkout-spec's default lets the \
                 bumper and the checkout disagree about WHICH repository the sha belongs \
                 to, and a sha from the wrong repository either fails oddly or resolves."
            ));
        }
    }

    // (10) The bumper is pointed at the pin file, and at exactly one file. Pointing
    //     it back at a workflow is not a no-op: `ci.yml` no longer has a 40-hex
    //     `ref:` below a spec-repository anchor, so the bumper's matcher would walk
    //     on and read an unrelated action pin -- `dtolnay/rust-toolchain`'s -- as
    //     the current spec ref. Measured: the rewrite then lands nowhere and the
    //     bumper fails its own post-rewrite assertion, so it is loud rather than
    //     destructive. Still wrong, and cheap to assert.
    let bump_files: Vec<&str> = bump_yml
        .lines()
        .filter_map(|l| l.trim_start().strip_prefix("file: "))
        .map(str::trim)
        .collect();
    if bump_files != [".spec-pin"] {
        out.push(format!(
            "invariant 10: bump-spec.yml passes {bump_files:?} to acdp-ci's bump-spec-ref, \
             expected exactly [\".spec-pin\"]. It bumps ONE file; pointing it at a \
             workflow again would have it read an unrelated action pin as the spec ref."
        ));
    }

    out
}

/// The real four files must satisfy all ten.
#[test]
fn the_spec_pin_stays_the_single_source_of_truth() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/<crate>/ is two levels below the workspace root")
        .to_path_buf();
    let read =
        |p: &str| std::fs::read_to_string(root.join(p)).unwrap_or_else(|e| panic!("read {p}: {e}"));

    let violations = spec_pin_violations(
        &read(".spec-pin"),
        &read(".github/workflows/ci.yml"),
        &read(".github/workflows/mutants.yml"),
        &read(".github/workflows/bump-spec.yml"),
    );
    assert!(
        violations.is_empty(),
        "the .spec-pin wiring is broken:\n  {}\n\nEvery consumer -- both workflows, \
         the spec bumper in another repository, and the conformance harness -- reads \
         that one file, and none of those couplings is visible from any single file. \
         mutants.yml is schedule-only, so without this test the damage would surface \
         on the next Monday cron rather than on the change that caused it.",
        violations.join("\n  ")
    );
}

/// Every invariant must FAIL on its own restructuring — otherwise the test above
/// is ten assertions that have never been shown to do anything, and an earlier
/// assertion masking a later one would be invisible.
///
/// The inputs are deliberately VALID YAML that a reasonable person would write.
/// None is a typo; each is a plausible edit that happens to break a coupling its
/// author could not see. Where an invariant applies to both workflows, BOTH are
/// falsified: the loop is shared code, but the inputs are not, and "the other file
/// is the same shape" is a hypothesis about a file nobody re-read.
#[test]
fn each_spec_pin_invariant_is_individually_falsified() {
    const GOOD_PIN: &str = "\
# A comment that DESCRIBES the anchor forms without spelling them.
repository: org/spec
ref: d1f06d0d49b73d411a3983d3877321ccaccd38e7
conformance-digest: rfc6962-sha256:03644a90cc643fd5bd5fe3c762389c16def704a874f94716619f7a949b965f85
";
    const GOOD_CI: &str = "\
jobs:
  conformance:
    steps:
      # checkout-spec@ is mentioned here in a comment on purpose.
      - uses: actions/checkout@1111111111111111111111111111111111111111
      - name: Read the pinned spec revision
        id: pin
        uses: ./.github/actions/read-spec-pin
      - uses: org/acdp-ci/actions/checkout-spec@2222222222222222222222222222222222222222 # v1
        with:
          repository: ${{ steps.pin.outputs.repository }}
          ref: ${{ steps.pin.outputs.ref }}
";
    const GOOD_MUTANTS: &str = "\
jobs:
  mutants:
    steps:
      - uses: actions/checkout@1111111111111111111111111111111111111111
      - name: Read the pinned spec revision
        id: pin
        uses: ./.github/actions/read-spec-pin
      - uses: org/acdp-ci/actions/checkout-spec@2222222222222222222222222222222222222222 # v1
        with:
          repository: ${{ steps.pin.outputs.repository }}
          ref: ${{ steps.pin.outputs.ref }}
      - uses: dtolnay/rust-toolchain@6c977a6ca4077a0ceb28ffbe03f59d46e9ac8772 # master
";
    const GOOD_BUMP: &str = "\
jobs:
  bump:
    uses: org/acdp-ci/.github/workflows/bump-spec-ref.yml@4444444444444444444444444444444444444444
    with:
      file: .spec-pin
";

    let good =
        |pin: &str, ci: &str, mut_: &str, bump: &str| spec_pin_violations(pin, ci, mut_, bump);

    // Control: the good quartet must pass, or every assertion below is vacuous.
    assert!(
        good(GOOD_PIN, GOOD_CI, GOOD_MUTANTS, GOOD_BUMP).is_empty(),
        "the control fixture must be clean: {:?}",
        good(GOOD_PIN, GOOD_CI, GOOD_MUTANTS, GOOD_BUMP)
    );

    let fires = |v: &[String], n: &str| v.iter().any(|s| s.starts_with(n));

    // (1) A second pin, e.g. someone adding a "previous" line for reference.
    let two_refs = format!("{GOOD_PIN}ref: 0000000000000000000000000000000000000000\n");
    let v = good(&two_refs, GOOD_CI, GOOD_MUTANTS, GOOD_BUMP);
    assert!(fires(&v, "invariant 1"), "two refs must trip 1, got {v:?}");
    let no_ref = GOOD_PIN.replace("ref: d1f06d0", "reff: d1f06d0");
    let v = good(&no_ref, GOOD_CI, GOOD_MUTANTS, GOOD_BUMP);
    assert!(fires(&v, "invariant 1"), "no ref must trip 1, got {v:?}");

    // (2) The declaration renamed -- the line shape `read-spec-pin` parses, gone.
    let no_repo = GOOD_PIN.replace("repository: org/spec", "spec-repository: org/spec");
    let v = good(&no_repo, GOOD_CI, GOOD_MUTANTS, GOOD_BUMP);
    assert!(
        fires(&v, "invariant 2"),
        "no repository declaration must trip 2, got {v:?}"
    );

    // (4) A comment that SPELLS the anchor form instead of describing it -- the
    //     single most likely way this file acquires a second anchor, and it makes
    //     the pin UNBUMPABLE rather than wrong, which is worse: nothing goes red.
    //     This case is why invariant 4 exists. It was written against invariant 2,
    //     it failed, and the failure was CORRECT -- a column-0 declaration count
    //     cannot see a comment, so the narrow check could not enforce the external
    //     contract its own message cited.
    let spelled = GOOD_PIN.replace(
        "# A comment that DESCRIBES the anchor forms without spelling them.",
        "# e.g. repository: org/spec",
    );
    let v = good(&spelled, GOOD_CI, GOOD_MUTANTS, GOOD_BUMP);
    assert!(
        fires(&v, "invariant 4"),
        "a spelled repository anchor in a comment must trip 4, got {v:?}"
    );
    assert!(
        !fires(&v, "invariant 2"),
        "and invariant 2 must stay SILENT on it -- if it fired too, the two are not \
         measuring different things and one is redundant: {v:?}"
    );

    // The other anchor form, and the one easier to write by accident: naming the
    // composite action in a comment. `ci.yml` mentions it three times, which is
    // precisely why the pin file must not mention it once.
    let spelled_action = GOOD_PIN.replace(
        "# A comment that DESCRIBES the anchor forms without spelling them.",
        "# the acdp-ci/actions/checkout-spec@ step in ci.yml consumes this",
    );
    let v = good(&spelled_action, GOOD_CI, GOOD_MUTANTS, GOOD_BUMP);
    assert!(
        fires(&v, "invariant 4"),
        "a spelled checkout-spec anchor in a comment must trip 4, got {v:?}"
    );

    // (3) The digest moved above the ref -- a tidy-looking reordering.
    const SWAPPED_PIN: &str = "\
repository: org/spec
conformance-digest: rfc6962-sha256:03644a90cc643fd5bd5fe3c762389c16def704a874f94716619f7a949b965f85
ref: d1f06d0d49b73d411a3983d3877321ccaccd38e7
";
    let v = good(SWAPPED_PIN, GOOD_CI, GOOD_MUTANTS, GOOD_BUMP);
    assert!(
        fires(&v, "invariant 3"),
        "digest above ref must trip 3, got {v:?}"
    );

    // (5) The repair that defeats the design, in EACH workflow independently.
    for (which, ci, mu) in [
        (
            "ci.yml",
            GOOD_CI.replace(
                "ref: ${{ steps.pin.outputs.ref }}",
                "ref: d1f06d0d49b73d411a3983d3877321ccaccd38e7",
            ),
            GOOD_MUTANTS.to_string(),
        ),
        (
            "mutants.yml",
            GOOD_CI.to_string(),
            GOOD_MUTANTS.replace(
                "ref: ${{ steps.pin.outputs.ref }}",
                "ref: d1f06d0d49b73d411a3983d3877321ccaccd38e7",
            ),
        ),
    ] {
        let v = good(GOOD_PIN, &ci, &mu, GOOD_BUMP);
        assert!(
            fires(&v, "invariant 5"),
            "a pasted literal ref in {which} must trip 5, got {v:?}"
        );
        assert!(
            v.iter().any(|s| s.contains(which)),
            "invariant 5 must name {which}, got {v:?}"
        );
    }

    // And its narrowness: `uses:` action pins ARE 40-hex shas -- GOOD_MUTANTS
    // carries one -- and must never be mistaken for a pasted spec pin. A guard
    // that banned every 40-hex string would fail against the correct file, which
    // is how a guard gets deleted rather than fixed.
    assert!(
        !fires(
            &good(GOOD_PIN, GOOD_CI, GOOD_MUTANTS, GOOD_BUMP),
            "invariant 5"
        ),
        "action `uses:` shas must not read as a pasted pin"
    );

    // (6) The reader step dropped, in each workflow.
    for (which, ci, mu) in [
        (
            "ci.yml",
            GOOD_CI.replace("        uses: ./.github/actions/read-spec-pin\n", ""),
            GOOD_MUTANTS.to_string(),
        ),
        (
            "mutants.yml",
            GOOD_CI.to_string(),
            GOOD_MUTANTS.replace("        uses: ./.github/actions/read-spec-pin\n", ""),
        ),
    ] {
        let v = good(GOOD_PIN, &ci, &mu, GOOD_BUMP);
        assert!(
            fires(&v, "invariant 6"),
            "a missing reader in {which} must trip 6, got {v:?}"
        );
    }

    // (7) A second spec checkout, in each workflow.
    for (which, ci, mu) in [
        ("ci.yml", format!("{GOOD_CI}      - uses: org/acdp-ci/actions/checkout-spec@3333333333333333333333333333333333333333\n"), GOOD_MUTANTS.to_string()),
        ("mutants.yml", GOOD_CI.to_string(), format!("{GOOD_MUTANTS}      - uses: org/acdp-ci/actions/checkout-spec@3333333333333333333333333333333333333333\n")),
    ] {
        let v = good(GOOD_PIN, &ci, &mu, GOOD_BUMP);
        assert!(
            fires(&v, "invariant 7"),
            "two spec checkouts in {which} must trip 7, got {v:?}"
        );
    }

    // (8) The reader moved below the checkout that consumes it -- valid YAML,
    //     green job, wrong tree. Built by reordering rather than by editing text,
    //     so the mutation cannot accidentally also break invariant 6.
    const READER_LAST_CI: &str = "\
jobs:
  conformance:
    steps:
      - uses: org/acdp-ci/actions/checkout-spec@2222222222222222222222222222222222222222 # v1
        with:
          repository: ${{ steps.pin.outputs.repository }}
          ref: ${{ steps.pin.outputs.ref }}
      - name: Read the pinned spec revision
        id: pin
        uses: ./.github/actions/read-spec-pin
";
    let v = good(GOOD_PIN, READER_LAST_CI, GOOD_MUTANTS, GOOD_BUMP);
    assert!(
        fires(&v, "invariant 8"),
        "a reader below the checkout must trip 8, got {v:?}"
    );
    // ...and ONLY 7, which is what proves the ordering assertion is doing the
    // work rather than inheriting a failure from a broken-in-two-ways fixture.
    assert!(
        !fires(&v, "invariant 6") && !fires(&v, "invariant 7"),
        "the reordering fixture must break ONLY the ordering invariant, got {v:?}"
    );

    // (9) `repository:` left to checkout-spec's default, in each workflow.
    for (which, ci, mu) in [
        (
            "ci.yml",
            GOOD_CI.replace(
                "          repository: ${{ steps.pin.outputs.repository }}\n",
                "",
            ),
            GOOD_MUTANTS.to_string(),
        ),
        (
            "mutants.yml",
            GOOD_CI.to_string(),
            GOOD_MUTANTS.replace(
                "          repository: ${{ steps.pin.outputs.repository }}\n",
                "",
            ),
        ),
    ] {
        let v = good(GOOD_PIN, &ci, &mu, GOOD_BUMP);
        assert!(
            fires(&v, "invariant 9"),
            "a dropped repository pass-through in {which} must trip 9, got {v:?}"
        );
    }

    // (10) The bumper pointed back at a workflow -- the regression U-536 undoes.
    let bump_ci = GOOD_BUMP.replace("file: .spec-pin", "file: .github/workflows/ci.yml");
    let v = good(GOOD_PIN, GOOD_CI, GOOD_MUTANTS, &bump_ci);
    assert!(
        fires(&v, "invariant 10"),
        "bumping a workflow must trip 10, got {v:?}"
    );
    // Two `file:` inputs is the other shape: the bumper takes one, so the second
    // is a copy nobody rewrites.
    let bump_two = GOOD_BUMP.replace(
        "      file: .spec-pin",
        "      file: .spec-pin\n      file: .spec-pin.old",
    );
    let v = good(GOOD_PIN, GOOD_CI, GOOD_MUTANTS, &bump_two);
    assert!(
        fires(&v, "invariant 10"),
        "two bump targets must trip 10, got {v:?}"
    );
}
