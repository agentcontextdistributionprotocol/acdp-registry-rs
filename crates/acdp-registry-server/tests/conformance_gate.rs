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

/// Wire codes the named producing functions in `src` can emit, extracted from
/// TEXT so that the guard over them is falsifiable here. For
/// `acdp-registry-types/src/error.rs` those are `wire_code`/`acdp_wire_code`; for
/// `acdp-registry-core/src/extract.rs` (the extractor rejections, which never
/// pass through `RegistryError`) they are `json_code`/`query_code`.
///
/// Scans ONLY the functions that produce wire codes, taking every string
/// literal inside them. Scanning `=> "..."` alone was the first attempt and it was
/// wrong: a long match pattern uses a block body, so
/// `SchemaViolation | InvalidBody | MissingField => { "schema_violation" }` has no
/// `=> "`. The count guard at the call site is what caught that.
fn wire_codes_in(src: &str, origin: &str, fn_names: &[&str]) -> Vec<String> {
    let mut codes: Vec<String> = Vec::new();
    for fn_name in fn_names {
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

/// The producing functions in `acdp-registry-types/src/error.rs`.
const ERROR_RS_CODE_FNS: [&str; 2] = ["fn wire_code", "fn acdp_wire_code"];

/// The producing functions in `acdp-registry-core/src/extract.rs`.
const EXTRACT_RS_CODE_FNS: [&str; 2] = ["fn json_code", "fn query_code"];

/// Wire codes `error.rs` and `extract.rs` emit today, as one de-duplicated set.
/// A ratchet, not a floor -- see the assertion that reads it for why an exact
/// number is affordable here, and for what the floor it replaced let through.
///
/// WAS 24, from `error.rs` alone. That left the extractor rejections unscanned,
/// and `unsupported_media_type` (the 415 minted in `extract.rs`, #247) appeared
/// nowhere in docs/HTTP-API.md while this guard stayed green.
const EXPECTED_WIRE_CODES: usize = 25;

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
    let found = wire_codes_in(SRC, "<synthetic>", &ERROR_RS_CODE_FNS);
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
    let fewer = wire_codes_in(&one_gone, "<synthetic>", &ERROR_RS_CODE_FNS);
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

    // The scanner reads WHICHEVER functions it is told to: with `extract.rs`'s
    // names it finds the extractor's codes, and with `error.rs`'s names it does
    // not -- which is how the 415 went unscanned before.
    const EXTRACT_SRC: &str = r#"
fn json_code(rej: &JsonRejection) -> &'static str {
    match rej {
        JsonRejection::MissingJsonContentType(_) => "unsupported_media_type",
        _ => "schema_violation",
    }
}
fn query_code(_rej: &QueryRejection) -> &'static str {
    "schema_violation"
}
"#;
    assert_eq!(
        wire_codes_in(EXTRACT_SRC, "<synthetic>", &EXTRACT_RS_CODE_FNS),
        vec![
            "unsupported_media_type".to_string(),
            "schema_violation".to_string()
        ]
    );
    assert!(
        !wire_codes_in(EXTRACT_SRC, "<synthetic>", &EXTRACT_RS_CODE_FNS[1..])
            .contains(&"unsupported_media_type".to_string()),
        "dropping `json_code` from the scanned set must lose the 415 code"
    );
    assert_eq!(
        code_field_literals(
            "Err(AcdpRejection { code: \"unsupported_media_type\", message: m })\n\
             let code: &str = x;\n"
        ),
        vec!["unsupported_media_type".to_string()],
        "only `code: \"...\"` struct-field literals count, not a `code:` binding"
    );
}

/// Every `code: "<wire_code>"` struct-field literal in `src` -- the rejections
/// `extract.rs` builds in place (`AcdpBytes`) rather than through `json_code`,
/// so a code minted only there is scanned too.
fn code_field_literals(src: &str) -> Vec<String> {
    src.split("code: \"")
        .skip(1)
        .filter_map(|tail| tail.split('"').next())
        .filter(|c| c.len() >= 4 && c.chars().all(|ch| ch.is_ascii_lowercase() || ch == '_'))
        .map(str::to_string)
        .collect()
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
/// `=> "wire_code"` arm in `acdp-registry-types/src/error.rs`, and every code the
/// extractor rejections in `acdp-registry-core/src/extract.rs` mint (they never
/// pass through `RegistryError`), must appear somewhere in `docs/HTTP-API.md`.
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
    let mut codes = wire_codes_in(&src, &error_rs.display().to_string(), &ERROR_RS_CODE_FNS);

    // The extractor rejections answer before any handler runs, so their codes
    // never reach `RegistryError::wire_code`. Scanned separately: both the
    // mapping functions and every `code: "..."` literal built in place.
    let extract_rs = root.join("crates/acdp-registry-core/src/extract.rs");
    let extract_src = std::fs::read_to_string(&extract_rs)
        .unwrap_or_else(|e| panic!("read {}: {e}", extract_rs.display()));
    let extract_codes: Vec<String> = wire_codes_in(
        &extract_src,
        &extract_rs.display().to_string(),
        &EXTRACT_RS_CODE_FNS,
    )
    .into_iter()
    .chain(code_field_literals(&extract_src))
    .collect();
    assert!(
        extract_codes.iter().any(|c| c == "unsupported_media_type"),
        "the extract.rs scan did not find `unsupported_media_type`, which \
         `json_code` and `AcdpBytes` certainly emit -- the scan is broken: \
         {extract_codes:?}"
    );
    for c in extract_codes {
        if !codes.contains(&c) {
            codes.push(c);
        }
    }
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
        "wire-code extraction found {} codes in {} and extract.rs, expected \
         exactly {EXPECTED_WIRE_CODES}. If you ADDED a wire code: update this constant and \
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
        "these wire codes can be emitted by acdp-registry-types/src/error.rs or \
         acdp-registry-core/src/extract.rs but appear NOWHERE in docs/HTTP-API.md: {undocumented:?}\n\
         Add them to the \"Status / code table\" with the HTTP status from \
         `http_status()` (`status_for_code` for an extract.rs code). A client cannot handle a code it has never been told \
         about, and a hand-kept table has no signal for the arm that was never \
         added — which is why this check is derived rather than maintained."
    );
}

/// The two shapes a hand-built error envelope takes in this codebase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EnvelopeSiteKind {
    /// A `"code":` JSON key -- `json!({"error": {"code": "..."}})`.
    JsonKey,
    /// A `code: "..."` struct-field literal -- `AcdpRejection { code: "..." }`.
    StructField,
}

/// Every line of `src` that mints a wire code by hand rather than through
/// `RegistryError` / `extract.rs`'s code functions: a `"code":` JSON key or a
/// `code: "` struct-field literal. Returns `(1-based line, kind)`.
///
/// Full-line comments (`//`, `///`, `//!`) are skipped, so prose that QUOTES an
/// envelope is not a site. Text, not a parse -- a pure function precisely so the
/// guard below can be falsified against synthetic source.
fn hand_built_envelope_sites(src: &str) -> Vec<(usize, EnvelopeSiteKind)> {
    fn followed_by(rest: &str, want: char) -> bool {
        rest.trim_start().starts_with(want)
    }
    let mut sites = Vec::new();
    for (i, line) in src.lines().enumerate() {
        if line.trim_start().starts_with("//") {
            continue;
        }
        if line
            .match_indices("\"code\"")
            .any(|(at, m)| followed_by(&line[at + m.len()..], ':'))
        {
            sites.push((i + 1, EnvelopeSiteKind::JsonKey));
        }
        if line.match_indices("code").any(|(at, m)| {
            let before_ok = !line[..at].chars().next_back().is_some_and(is_ident_char);
            let after = line[at + m.len()..].trim_start();
            before_ok
                && after
                    .strip_prefix(':')
                    .is_some_and(|tail| !tail.starts_with(':') && followed_by(tail, '"'))
        }) {
            sites.push((i + 1, EnvelopeSiteKind::StructField));
        }
    }
    sites
}

/// The one file allowed to build a `code: "..."` literal in place: the
/// extractor rejections, whose codes `every_wire_code_the_code_emits_is_documented`
/// scans via `code_field_literals`.
const STRUCT_FIELD_CODE_ALLOWED_IN: &str = "crates/acdp-registry-core/src/extract.rs";

/// #376: `POST /auth/token/revoke` hand-built
/// `json!({"error": {"code": "service_unavailable"}})` -- a 503 with a code that
/// is in no RFC-ACDP-0007 §5 table and that `every_wire_code_the_code_emits_is_documented`
/// could not see, because that guard scans only `error.rs` and `extract.rs`. Its
/// exact count (`EXPECTED_WIRE_CODES`) is only as good as the claim that those
/// two files are where codes come from; this test is what makes that claim
/// checked rather than assumed.
///
/// Walks every `crates/*/src` file, drops `#[cfg(test)]` regions (unstripped if
/// `strip_cfg_test` declines -- the loud direction), and fails on any `"code":`
/// key anywhere and on any `code: "` literal outside `extract.rs`.
#[test]
fn no_wire_code_is_minted_outside_the_scanned_files() {
    // Falsify the scanner first, against synthetic source.
    let clean = "fn f() -> Result<(), RegistryError> {\n    Err(RegistryError::Acdp(e))\n}\n";
    assert!(
        hand_built_envelope_sites(clean).is_empty(),
        "clean source must yield no site"
    );
    let dirty = clean.replace(
        "    Err(RegistryError::Acdp(e))\n",
        "    Json(json!({\"error\":{\"code\":\"x\"}}))\n",
    );
    assert_eq!(
        hand_built_envelope_sites(&dirty),
        vec![(2, EnvelopeSiteKind::JsonKey)],
        "inserting one hand-built envelope must yield exactly one hit"
    );
    assert_eq!(
        hand_built_envelope_sites("    let r = AcdpRejection { code: \"x\", message };\n"),
        vec![(1, EnvelopeSiteKind::StructField)],
        "a `code: \"` struct-field literal is a site"
    );
    assert!(
        hand_built_envelope_sites(
            "/// answers `{\"code\": \"x\"}`\nlet code: &str = c;\nfoo::code::bar(\"y\");\nerrcode: \"z\"\n"
        )
        .is_empty(),
        "comments, a `code:` binding, a `code::` path and a longer identifier are not sites"
    );

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/<crate>/ is two levels below the workspace root")
        .to_path_buf();
    let mut files: Vec<(std::path::PathBuf, String)> = Vec::new();
    for crate_dir in std::fs::read_dir(root.join("crates"))
        .expect("read crates/")
        .flatten()
    {
        let src = crate_dir.path().join("src");
        if src.is_dir() {
            rust_source_files_with_paths(&src, &mut files);
        }
    }
    // Same enumeration check as the env-var sweep: a walk that silently drops
    // files would make "no hits" vacuous.
    let tracked_src = git_tracked_paths(&root)
        .into_iter()
        .filter(|p| p.starts_with("crates/") && p.ends_with(".rs") && p.contains("/src/"))
        .count();
    assert_eq!(
        files.len(),
        tracked_src,
        "the crates/*/src walk found {} .rs files and git tracks {}",
        files.len(),
        tracked_src
    );

    let mut violations: Vec<String> = Vec::new();
    let mut allowed_struct_fields = 0usize;
    for (path, text) in &files {
        let rel = path
            .strip_prefix(&root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        let scanned = strip_cfg_test(text).unwrap_or_else(|| text.clone());
        for (line, kind) in hand_built_envelope_sites(&scanned) {
            if kind == EnvelopeSiteKind::StructField && rel == STRUCT_FIELD_CODE_ALLOWED_IN {
                allowed_struct_fields += 1;
                continue;
            }
            // Line numbers are of the cfg(test)-stripped text; the path is exact.
            violations.push(format!("{rel} (stripped line {line}): {kind:?}"));
        }
    }
    assert!(
        allowed_struct_fields > 0,
        "found no `code: \"` literal in {STRUCT_FIELD_CODE_ALLOWED_IN}, which builds \
         `unsupported_media_type` and `payload_too_large` in place -- the scan is broken"
    );
    assert!(
        violations.is_empty(),
        "these sites mint a wire code by hand, outside error.rs/extract.rs, so \
         `every_wire_code_the_code_emits_is_documented` cannot see the code: \
         {violations:#?}\n\
         Return a `RegistryError` (its `wire_code`/`http_status` table is the one \
         scanned) instead of building the envelope in place."
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
    let mut with_paths = Vec::new();
    rust_source_files_with_paths(dir, &mut with_paths);
    out.extend(with_paths.into_iter().map(|(_, text)| text));
}

/// [`rust_source_file_texts`], keeping each file's path -- for guards whose
/// failure message has to say WHERE, not just what.
fn rust_source_files_with_paths(
    dir: &std::path::Path,
    out: &mut Vec<(std::path::PathBuf, String)>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == "target") {
                continue;
            }
            rust_source_files_with_paths(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            if let Ok(s) = std::fs::read_to_string(&path) {
                out.push((path, s));
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

// ---------------------------------------------------------------------------
// THE SURVIVOR CLASSIFIER'S WIRING.
//
// Same verification gap as `spec_pin_violations` above, and worth restating
// because it is the whole reason this logic is a committed script rather than
// more shell inside the workflow. `mutants.yml` is `schedule:` +
// `workflow_dispatch` with NO `pull_request` trigger, so a PR that deletes the
// classifier call, renames the script, or points MUTANTS_PRIOR_LEDGER at a glob
// cannot turn that job red. The damage surfaces on the next Monday's cron,
// detached from the change that caused it -- and in this case it surfaces as a
// ratchet that quietly went back to telling readers "Usually GOOD NEWS, delete
// the line" about mutants that merely moved.
//
// The GLOB check is not hypothetical. `docs/mutation-runs/` holds a VOID ledger
// (measured with `copy_vcs` lost, so every "caught" in it is an artifact) and
// three SUPERSEDED shards. Globbing that directory yields 13 files, 417 records,
// 66 duplicate names and 11 with CONFLICTING verdicts -- which the classifier
// would (correctly) refuse to classify against, turning the ratchet into a
// permanent hard failure nobody could act on.
// ---------------------------------------------------------------------------

const CLASSIFIER: &str = ".github/scripts/classify_removed_survivors.py";

/// The value of a top-level `KEY: "value"` env entry, unquoted.
fn yaml_scalar<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines()
        .map(str::trim)
        .find_map(|l| l.strip_prefix(key)?.strip_prefix(':'))
        .map(|v| v.trim().trim_matches('"').trim_matches('\''))
}

fn classifier_wiring_violations(mutants_yml: &str) -> Vec<String> {
    let mut v = Vec::new();

    if !mutants_yml.contains(CLASSIFIER) {
        v.push(format!(
            "mutants.yml never invokes {CLASSIFIER}. A disappeared survivor line is \
             then reported with no evidence of whether it was KILLED or merely DRIFTED."
        ));
    }
    // It must read the FULL report. `missed.txt` is the survivor subset and is
    // exactly what cannot answer the question.
    if !mutants_yml.contains("--outcomes") {
        v.push(
            "the classifier is invoked without --outcomes, so it cannot read the full \
             verdict map that makes a kill provable."
                .to_string(),
        );
    }
    match yaml_scalar(mutants_yml, "MUTANTS_PRIOR_LEDGER") {
        None => v.push(
            "MUTANTS_PRIOR_LEDGER is not declared, so a drifted survivor cannot be \
             paired to its new location and every drift degrades to a coarse guess."
                .to_string(),
        ),
        Some("") => v.push("MUTANTS_PRIOR_LEDGER is declared but empty.".to_string()),
        Some(p) if p.contains('*') || p.contains('?') => v.push(format!(
            "MUTANTS_PRIOR_LEDGER is a GLOB ({p:?}). It must name exactly ONE ledger: \
             docs/mutation-runs/ also holds a VOID ledger and three SUPERSEDED shards, \
             whose verdicts contradict the current ones."
        )),
        Some(_) => {}
    }
    if !mutants_yml.contains("--prior-ledger") {
        v.push(
            "MUTANTS_PRIOR_LEDGER is declared but never passed as --prior-ledger, so it \
             documents an intent the run does not act on."
                .to_string(),
        );
    }
    if !mutants_yml.contains("--removed-file") {
        v.push(
            "the classifier is invoked without --removed-file, so it is given nothing to \
             classify."
                .to_string(),
        );
    }
    // A SHARDED report classifies every other shard's survivors as deleted, and
    // only the scope pin can tell a shard from a whole run.
    if !mutants_yml.contains("--expected-scope") {
        v.push(
            "the classifier is invoked without --expected-scope, so a SHARDED report \
             would be classified as if it were the whole run -- reporting every survivor \
             belonging to another shard as one whose expression was deleted."
                .to_string(),
        );
    }
    // Without this the entire prior-ledger FRESHNESS gate can be disabled by
    // deleting one flag, while every other wiring check stays green.
    if !mutants_yml.contains("--committed-file") {
        v.push(
            "the classifier is invoked without --committed-file, so a STALE prior ledger \
             is never detected: a MUTANTS_SURVIVORS line edited without committing a \
             matching ledger would silently degrade every drift to a coarse guess."
                .to_string(),
        );
    }
    // The exit codes are 10/11 precisely so a crash (1) or an argparse error (2)
    // cannot be read as a normal classification. Handling that distinction is the
    // point; dropping it re-hides a broken classifier.
    if !mutants_yml.contains("the classifier itself failed") {
        v.push(
            "the workflow does not distinguish a CRASHED classifier from a normal \
             classification. The script exits 10/11 on purpose so that 1 (unhandled \
             exception) and 2 (argparse) stay distinguishable; without that branch a \
             crash reads as agreement."
                .to_string(),
        );
    }
    // The raw list must be printed by the WORKFLOW, before the classifier runs --
    // otherwise a missing or crashing script leaves the operator with no idea
    // which lines vanished.
    if !mutants_yml.contains("listed as surviving but NO LONGER in missed.txt:") {
        v.push(
            "the workflow no longer prints the removed lines itself. A diagnostic that \
             exists only inside the tool being diagnosed disappears exactly when the \
             tool breaks."
                .to_string(),
        );
    }
    v
}

#[test]
fn the_declared_prior_ledger_actually_exists() {
    // MUTANTS_PRIOR_LEDGER is a PATH, and nothing else checks that it resolves.
    // The ratchet fails closed if it dangles -- the classifier cannot open the
    // file, exits non-10/11, and the workflow reports "the classifier itself
    // failed" -- but only on the rare branch where a committed survivor vanishes.
    // Between the rename and that branch firing, the gate looks healthy and its
    // drift evidence is silently unavailable. The pointer is re-aimed every time
    // the scope is re-measured (U-551 -> U-552 renamed it), so this is a live
    // hazard, not a hypothetical one. Checking existence here moves detection to
    // the PR that breaks it, on the required `tests` context.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/<crate>/ is two levels below the workspace root")
        .to_path_buf();
    let mutants_yml = std::fs::read_to_string(root.join(".github/workflows/mutants.yml"))
        .expect("read .github/workflows/mutants.yml");

    let ledger = yaml_scalar(&mutants_yml, "MUTANTS_PRIOR_LEDGER")
        .expect("mutants.yml declares MUTANTS_PRIOR_LEDGER");
    assert!(
        !ledger.is_empty(),
        "MUTANTS_PRIOR_LEDGER is declared but empty"
    );
    assert!(
        root.join(ledger).is_file(),
        "MUTANTS_PRIOR_LEDGER points at {ledger:?}, which is not a file in this repo.          A dangling ledger pointer does not fail until a committed survivor vanishes,          which may be many PRs after the rename that broke it."
    );

    // ON DISK IS NOT ENOUGH -- it must be TRACKED. CI checks out the commit, so a
    // ledger that exists only in someone's working tree is absent there. Testing
    // `is_file()` alone passes locally for the author and fails for everyone else,
    // which is the worst shape a gate can have: green where it is written, red
    // where it is enforced. `git ls-files` is a second enumeration that an
    // unstaged file cannot satisfy.
    let tracked = std::process::Command::new("git")
        .args(["ls-files", "--error-unmatch", "--", ledger])
        .current_dir(&root)
        .output()
        .unwrap_or_else(|e| panic!("could not run `git ls-files` in {}: {e}", root.display()));
    assert!(
        tracked.status.success(),
        "MUTANTS_PRIOR_LEDGER points at {ledger:?}, which exists on disk but is NOT \
         tracked by git. CI checks out the commit, so the ratchet would find no ledger \
         there while this test passes locally. `git add` it."
    );
}

// ---------------------------------------------------------------------------
// THE PRIOR LEDGER MUST CONTAIN EVERY COMMITTED SURVIVOR (#371).
//
// This is the classifier's FRESHNESS rule (`classify_removed_survivors.py`, the
// `STALE PRIOR LEDGER` branch), moved to PR time. Inside the scheduled job that
// rule only fires when a survivor disappears AND needs pairing. A stale ledger can
// therefore sit unnoticed for weeks, and the first drift then gets "cannot
// conclude" instead of an answer.
//
// It already happened. #331 moved the store.rs survivor from :1306:35 to :1317:35
// in MUTANTS_SURVIVORS without committing a new ledger, and u552 kept the old
// name until #371. This test would have failed #331.
//
// The strip rules are the workflow's (`survivors_expected` in mutants.yml's
// ratchet step): trailing whitespace trimmed, `#` lines and blank lines dropped.
// Membership is by `scenario.Mutant.name`, the field the classifier reads.
// ---------------------------------------------------------------------------

/// The committed survivor lines from the `MUTANTS_SURVIVORS: |` block scalar,
/// with the workflow's strip rules applied. `None` if the key is absent.
fn committed_survivors(mutants_yml: &str) -> Option<Vec<String>> {
    let mut lines = mutants_yml.lines();
    let header = lines
        .by_ref()
        .find(|l| l.trim_start().starts_with("MUTANTS_SURVIVORS:"))?;
    let key_indent = header.len() - header.trim_start().len();
    let mut out = Vec::new();
    for line in lines {
        let indent = line.len() - line.trim_start().len();
        // A block scalar ends at the first non-blank line indented no deeper
        // than its key.
        if !line.trim().is_empty() && indent <= key_indent {
            break;
        }
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        out.push(l.to_string());
    }
    Some(out)
}

/// Committed survivor lines that are NOT a `scenario.Mutant.name` in the
/// ledger. `Err` when either input cannot support a conclusion.
fn survivors_missing_from_ledger(
    mutants_yml: &str,
    ledger_json: &str,
) -> Result<Vec<String>, String> {
    let survivors =
        committed_survivors(mutants_yml).ok_or("mutants.yml does not declare MUTANTS_SURVIVORS")?;
    if survivors.is_empty() {
        // An empty list makes "every survivor is in the ledger" vacuously true.
        return Err("MUTANTS_SURVIVORS parsed to zero lines".to_string());
    }
    let doc: serde_json::Value =
        serde_json::from_str(ledger_json).map_err(|e| format!("ledger is not JSON: {e}"))?;
    let outcomes = doc
        .get("outcomes")
        .and_then(|o| o.as_array())
        .ok_or("ledger has no `outcomes` array; the report format changed")?;
    // `scenario` is the bare string "Baseline" for the baseline record, so the
    // object lookup has to tolerate a non-object.
    let names: std::collections::HashSet<&str> = outcomes
        .iter()
        .filter_map(|o| o.get("scenario")?.get("Mutant")?.get("name")?.as_str())
        .collect();
    if names.is_empty() {
        return Err("ledger contains no mutant records".to_string());
    }
    Ok(survivors
        .into_iter()
        .filter(|s| !names.contains(s.as_str()))
        .collect())
}

#[test]
fn every_committed_survivor_is_in_the_prior_ledger() {
    let root = repo_root();
    let mutants_yml = std::fs::read_to_string(root.join(".github/workflows/mutants.yml"))
        .expect("read .github/workflows/mutants.yml");
    let ledger = yaml_scalar(&mutants_yml, "MUTANTS_PRIOR_LEDGER")
        .expect("mutants.yml declares MUTANTS_PRIOR_LEDGER");
    let ledger_json = std::fs::read_to_string(root.join(ledger))
        .unwrap_or_else(|e| panic!("read MUTANTS_PRIOR_LEDGER {ledger}: {e}"));

    let missing = survivors_missing_from_ledger(&mutants_yml, &ledger_json)
        .unwrap_or_else(|e| panic!("cannot check the prior ledger's freshness: {e}"));
    assert!(
        missing.is_empty(),
        "STALE PRIOR LEDGER: {} committed MUTANTS_SURVIVORS line(s) are not mutant names in \
         {ledger}:\n  {}\n\nThe scheduled ratchet pairs a drifted survivor against this \
         ledger, and the classifier refuses a ledger missing any committed line. Commit the \
         outcomes.json of the run that produced the current MUTANTS_SURVIVORS and point \
         MUTANTS_PRIOR_LEDGER at it, in the same commit as the survivor-list edit.",
        missing.len(),
        missing.join("\n  ")
    );
}

/// The guard above must FAIL on a stale ledger, and must not pass vacuously.
#[test]
fn the_prior_ledger_freshness_check_is_falsified() {
    const A: &str = "crates/x/src/a.rs:10:5: replace > with >= in f";
    const B: &str = "crates/x/src/b.rs:20:9: delete ! in g";
    let yml = format!(
        "env:\n  MUTANTS_PRIOR_LEDGER: \"l.json\"\n\n  MUTANTS_SURVIVORS: |\n    \
         # reason for A\n    {A}\n\n    # reason for B\n    {B}   \n\n  \
         MUTANTS_TIMEOUT_BUDGET: \"1\"\n"
    );
    let ledger = |names: &[&str]| {
        let mut outcomes = vec![serde_json::json!({"scenario": "Baseline", "summary": "Success"})];
        outcomes.extend(names.iter().map(
            |n| serde_json::json!({"scenario": {"Mutant": {"name": n}}, "summary": "MissedMutant"}),
        ));
        serde_json::json!({ "outcomes": outcomes }).to_string()
    };

    // The parser must see exactly the two survivors: comments, blanks and
    // trailing whitespace stripped, and the block ending at the next key.
    assert_eq!(
        committed_survivors(&yml),
        Some(vec![A.to_string(), B.to_string()]),
        "the survivor parser does not apply the workflow's strip rules"
    );

    // Control: a fresh ledger passes.
    assert_eq!(
        survivors_missing_from_ledger(&yml, &ledger(&[A, B, "crates/x/src/c.rs:1:1: other"])),
        Ok(vec![]),
        "the control (fresh ledger) must be clean, or the cases below prove nothing"
    );

    // The #331 shape: one survivor line moved and the ledger still has the old name.
    let stale = ledger(&[A, "crates/x/src/b.rs:9:9: delete ! in g"]);
    assert_eq!(
        survivors_missing_from_ledger(&yml, &stale),
        Ok(vec![B.to_string()]),
        "a ledger missing a committed survivor was not reported, or the wrong line was named"
    );

    // Vacuity: no survivors, no ledger records, or no outcomes array must not pass.
    let empty_list = "env:\n  MUTANTS_SURVIVORS: |\n    # nothing\n  NEXT: \"x\"\n";
    assert!(survivors_missing_from_ledger(empty_list, &ledger(&[A])).is_err());
    assert!(survivors_missing_from_ledger("env:\n  OTHER: \"x\"\n", &ledger(&[A])).is_err());
    assert!(survivors_missing_from_ledger(&yml, &ledger(&[])).is_err());
    assert!(survivors_missing_from_ledger(&yml, "{\"total_mutants\": 2}").is_err());
}

#[test]
fn the_survivor_classifier_is_still_wired_into_the_ratchet() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/<crate>/ is two levels below the workspace root")
        .to_path_buf();
    let read =
        |p: &str| std::fs::read_to_string(root.join(p)).unwrap_or_else(|e| panic!("read {p}: {e}"));
    let mutants_yml = read(".github/workflows/mutants.yml");

    let violations = classifier_wiring_violations(&mutants_yml);
    assert!(
        violations.is_empty(),
        "the survivor classifier's wiring is broken:\n  {}\n\nmutants.yml has no \
         pull_request trigger, so without this test the breakage would surface on the \
         next Monday cron rather than on the change that caused it.",
        violations.join("\n  ")
    );

    // The script and its tests must actually exist -- a wiring check that passes
    // while the target is missing is the failure this whole unit is about.
    for p in [
        CLASSIFIER,
        ".github/scripts/test_classify_removed_survivors.py",
    ] {
        assert!(
            root.join(p).is_file(),
            "{p} is referenced but does not exist"
        );
    }

    // And the ledger it names must exist, or every drift silently degrades to the
    // coarse fallback while the workflow still looks correctly configured.
    let ledger = yaml_scalar(&mutants_yml, "MUTANTS_PRIOR_LEDGER").expect("checked above");
    assert!(
        root.join(ledger).is_file(),
        "MUTANTS_PRIOR_LEDGER names {ledger}, which does not exist. Drift pairing would \
         fall back to a coarse (file, description) match that cannot tell sibling \
         mutants apart -- and in this repo three `delete !` mutants in one function hold \
         three DIFFERENT verdicts."
    );
}

/// Every invariant above must FAIL on its own breakage, or the test is four
/// assertions that have never been shown to do anything.
#[test]
fn each_classifier_wiring_invariant_is_individually_falsified() {
    // FALSIFIED AGAINST THE REAL FILE FIRST. A hand-written control can only
    // contain the shapes its author thought of; the shipped workflow is the input
    // that actually has to survive these checks.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/<crate>/ is two levels below the workspace root")
        .to_path_buf();
    let real = std::fs::read_to_string(root.join(".github/workflows/mutants.yml"))
        .expect("read mutants.yml");
    for (label, needle) in [
        ("call deleted", CLASSIFIER),
        ("--outcomes dropped", "--outcomes"),
        ("--removed-file dropped", "--removed-file"),
        ("--expected-scope dropped", "--expected-scope"),
        ("--prior-ledger dropped", "--prior-ledger"),
        ("--committed-file dropped", "--committed-file"),
        ("crash branch dropped", "the classifier itself failed"),
        (
            "workflow stops printing the list",
            "listed as surviving but NO LONGER in missed.txt:",
        ),
    ] {
        let broken = real.replace(needle, "# removed by falsification #");
        assert!(
            !classifier_wiring_violations(&broken).is_empty(),
            "breaking the REAL mutants.yml ({label}) produced no violation, so that \
             invariant is decorative"
        );
    }
    // A glob is the one break that is an EDIT rather than a deletion.
    //
    // The ledger path is READ OUT of the real file rather than written here as a
    // literal. A literal pins this test to one ledger filename, and ledgers are
    // renamed every time the scope is re-measured -- U-552 renamed it from
    // `u551-core-scope-213` to `u552-union-scope-351`. With a literal, that
    // rename makes `replace` a silent no-op, so `globbed == real`, no violation
    // is produced, and the assertion fires with the message "so that invariant is
    // decorative" -- convicting the gate of a defect that is really in this test.
    // A falsification whose mutation never applied does not prove the invariant
    // is dead; it proves nothing at all, which is the more dangerous of the two.
    let ledger = yaml_scalar(&real, "MUTANTS_PRIOR_LEDGER")
        .expect("mutants.yml declares MUTANTS_PRIOR_LEDGER");
    assert!(
        !ledger.is_empty() && !ledger.contains('*'),
        "MUTANTS_PRIOR_LEDGER is {ledger:?}, which is empty or already a glob --          the glob falsification below cannot mean anything against it"
    );
    let globbed = real.replace(ledger, "docs/mutation-runs/*-outcomes.json");
    assert_ne!(
        globbed, real,
        "substituting the ledger path {ledger:?} changed nothing, so the glob          falsification never ran. Fix THIS test, not the gate."
    );
    assert!(
        !classifier_wiring_violations(&globbed).is_empty(),
        "pointing the REAL mutants.yml at a ledger GLOB produced no violation"
    );

    let good = format!(
        "env:\n  MUTANTS_EXPECTED_SCOPE: \"213\"\n  \
         MUTANTS_PRIOR_LEDGER: \"docs/mutation-runs/u551-core-scope-213-outcomes.json\"\n\
         jobs:\n  mutants:\n    steps:\n      - run: |\n          python3 {CLASSIFIER} \
         --outcomes \"$json\" --removed-file r.txt --prior-ledger \"$MUTANTS_PRIOR_LEDGER\" \
         --expected-scope 213 --committed-file c.txt\n          echo 'listed as surviving but NO LONGER in \
         missed.txt:'\n          echo 'the classifier itself failed'\n"
    );
    assert!(
        classifier_wiring_violations(&good).is_empty(),
        "the control input must be clean, or every case below proves nothing"
    );

    // Each break is a plausible edit, not a typo, and each must be caught ALONE.
    let cases: Vec<(&str, String)> = vec![
        ("call deleted", good.replace(CLASSIFIER, "echo skipped #")),
        ("--outcomes dropped", good.replace("--outcomes \"$json\" ", "")),
        (
            "ledger undeclared",
            good.replace("  MUTANTS_PRIOR_LEDGER: \"docs/mutation-runs/u551-core-scope-213-outcomes.json\"\n", ""),
        ),
        (
            "ledger is a glob",
            good.replace(
                "docs/mutation-runs/u551-core-scope-213-outcomes.json",
                "docs/mutation-runs/*-outcomes.json",
            ),
        ),
        ("declared but never passed", good.replace(" --prior-ledger \"$MUTANTS_PRIOR_LEDGER\"", "")),
    ];
    for (label, broken) in cases {
        assert!(
            !classifier_wiring_violations(&broken).is_empty(),
            "breaking the wiring ({label}) produced NO violation, so that invariant is \
             decorative:\n{broken}"
        );
    }
}

// ---- D3 (hardening-remaining Phase 8): doc-truth guards --------------------

fn repo_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/<crate>/ is two levels below the workspace root")
        .to_path_buf()
}

/// Every line-number citation in `text`, as `"<lineno>: <line>"`: a
/// `<file>.rs:<digits>` pin, or a bare continuation pin `` `:<digits>` `` (the
/// "(`:122`, `:128`)" form that cites lines of a file named earlier in the
/// sentence). A continuation pin is all digits up to the closing backtick, so a
/// version-shaped image tag (`` `:0.2` ``, `` `:0.2.0` ``) or `` `:latest` `` is
/// not one. A port must be written with its host (`localhost:8080`), not bare.
fn rs_line_pins(text: &str) -> Vec<String> {
    let continuation_pin = |tail: &str| {
        let digits = tail.chars().take_while(char::is_ascii_digit).count();
        digits > 0 && tail[digits..].starts_with('`')
    };
    text.lines()
        .enumerate()
        .filter(|(_, l)| {
            l.split(".rs:")
                .skip(1)
                .any(|tail| tail.starts_with(|c: char| c.is_ascii_digit()))
                || l.split("`:").skip(1).any(continuation_pin)
        })
        .map(|(i, l)| format!("{}: {}", i + 1, l.trim()))
        .collect()
}

/// Operator-facing files that may cite source, but never by line number.
/// Excluded on purpose: `docs/ENGINEERING-LOG.md` (a dated narrative whose
/// citations describe the tree AS IT WAS when each entry was written) and
/// `docs/MUTATION-SCOPE-CANDIDATES.md` (a point-in-time mutation-testing
/// survey that pins the lines its survivors were found on).
fn line_pin_scope(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    const EXCLUDED: [&str; 2] = ["ENGINEERING-LOG.md", "MUTATION-SCOPE-CANDIDATES.md"];
    let list = |dir: &str| -> Vec<std::path::PathBuf> {
        let mut v: Vec<_> = std::fs::read_dir(root.join(dir))
            .unwrap_or_else(|e| panic!("read_dir {dir}: {e}"))
            .map(|e| e.expect("dir entry").path())
            .filter(|p| p.is_file())
            .collect();
        v.sort();
        v
    };
    let mut files = vec![root.join("README.md")];
    files.extend(list("docs").into_iter().filter(|p| {
        let name = p.file_name().unwrap().to_string_lossy().into_owned();
        name.ends_with(".md") && !EXCLUDED.contains(&name.as_str())
    }));
    files.extend(
        list("config")
            .into_iter()
            .filter(|p| p.extension().is_some_and(|e| e == "toml")),
    );
    files.extend(list("docker"));
    files
}

/// D3 guard (a). `file.rs:N` citations in operator-facing text rotted
/// repeatedly: Phase 8 of the hardening plan found 17 of them, several pointing
/// at unrelated code after ordinary edits (`main.rs:1187` for
/// `supported_signature_algorithms`, which had moved ~140 lines). Cite the
/// symbol instead — a renamed symbol fails a grep, a moved line fails nothing.
/// Same rule `authentication_doc_cites_symbols_that_exist_and_never_line_numbers`
/// enforces for AUTHENTICATION.md, widened to every operator-facing file.
///
/// Also scans `::error::` lines in tracked `.github/` files (their text is what
/// a CI reader acts on). LIMIT, stated: an `::error::` message continued onto a
/// second line is checked only on its first; `docker/` scripts are scanned
/// whole, so the gap is `.github/` only. Plain `.github/` comments are out of
/// scope.
#[test]
fn operator_docs_never_cite_source_by_line_number() {
    let root = repo_root();
    let files = line_pin_scope(&root);
    // Vacuity guard: the scope must include the files the drift was found in,
    // or a broken directory listing would pass everything.
    for must in [
        "README.md",
        "docs/HTTP-API.md",
        "docs/CONFIGURATION.md",
        "docs/OPERATIONS.md",
        "config/registry.example.toml",
        "docker/RAILWAY.md",
        "docker/assert-quickstart-boots.sh",
        "docker/compose.ci-auth-on.yml",
    ] {
        assert!(
            files.contains(&root.join(must)),
            "line-pin scope lost {must}: {files:?}"
        );
    }
    assert!(
        !files.iter().any(|p| p.ends_with("docs/ENGINEERING-LOG.md")),
        "the historical log must stay out of scope"
    );

    let mut found = Vec::new();
    for f in &files {
        let text = std::fs::read_to_string(f).unwrap_or_else(|e| panic!("{}: {e}", f.display()));
        for pin in rs_line_pins(&text) {
            found.push(format!(
                "{}:{pin}",
                f.strip_prefix(&root).unwrap().display()
            ));
        }
    }
    let gh: Vec<String> = git_tracked_paths(&root)
        .into_iter()
        .filter(|p| p.starts_with(".github/"))
        .collect();
    assert!(
        gh.iter().any(|p| p == ".github/workflows/docker.yml"),
        "git ls-files returned no .github workflows: {gh:?}"
    );
    for p in gh {
        let Ok(text) = std::fs::read_to_string(root.join(&p)) else {
            continue;
        };
        for line in text.lines().filter(|l| l.contains("::error::")) {
            if !rs_line_pins(line).is_empty() {
                found.push(format!("{p}: {}", line.trim()));
            }
        }
    }
    assert!(
        found.is_empty(),
        "operator-facing text cites source by line number:\n  {}\n\
         Line pins rot silently on unrelated edits. Cite the function, constant or \
         test by name instead (e.g. `build_capabilities` in \
         `crates/acdp-registry-server/src/main.rs`).",
        found.join("\n  ")
    );

    // Negative control for the matcher itself.
    assert_eq!(
        rs_line_pins("see `main.rs:1187` here\nno pin in `main.rs`\n").len(),
        1
    );
    // Continuation pins are caught; version tags, `:latest` and `::error::` are not.
    assert_eq!(
        rs_line_pins(
            "trimmed before comparison (`:122`, `:128`).\n\
             pin `:0.2.0`, or `:0.2`, never `:latest`\n\
             echo \"`::error::` text\"\n\
             at `:7`\n"
        ),
        vec![
            "1: trimmed before comparison (`:122`, `:128`).".to_string(),
            "4: at `:7`".to_string(),
        ]
    );
}

/// D3 guard (b). `docker/RAILWAY.md` told operators to deploy `:0.1` after
/// 0.2.0 shipped. Every image version tag on the page must share the
/// major.minor of the release the page is documenting: the newest `## X.Y.Z`
/// section of `docs/UPGRADING.md`, or the workspace version when that is
/// newer (see [`expected_mm`]). release-plz owns the version bump and tags
/// `acdp-registry-server/v<version>`, which docker.yml turns into the image
/// tags; a breaking 0.x release has its UPGRADING section merged on `main`
/// before the bump, so the page must already name that minor, and once the
/// release lands the two coincide. Compared at major.minor so a patch release
/// does not redden the release PR.
#[test]
fn railway_image_tags_track_the_workspace_version() {
    let root = repo_root();
    let version = env!("CARGO_PKG_VERSION");
    let upgrading =
        std::fs::read_to_string(root.join("docs/UPGRADING.md")).expect("read UPGRADING.md");
    let headings = upgrading_versions(&upgrading);
    assert!(
        !headings.is_empty(),
        "no `## X.Y.Z` section found in docs/UPGRADING.md; the heading scanner or the page changed shape"
    );
    let mm = expected_mm(version, &upgrading);
    assert!(
        headings.iter().any(|h| format!("{}.{}", h.0, h.1) == mm),
        "expected major.minor {mm} (workspace {version}) is not an UPGRADING section"
    );
    let doc = std::fs::read_to_string(root.join("docker/RAILWAY.md")).expect("read RAILWAY.md");

    let tags = railway_version_tags(&doc);
    assert!(
        tags.len() >= 4,
        "found only {} version tags in docker/RAILWAY.md ({tags:?}); the scanner \
         or the page changed shape",
        tags.len()
    );
    let stale: Vec<&String> = tags
        .iter()
        .filter(|t| t.as_str() != mm && !t.starts_with(&format!("{mm}.")))
        .collect();
    assert!(
        stale.is_empty(),
        "docker/RAILWAY.md names image tags {stale:?}, but the release being documented is \
         {mm} (workspace {version}, newest docs/UPGRADING.md section): operators following \
         the page would deploy an old minor. Update every version tag on the page to \
         {mm} / {mm}.<patch>."
    );
    assert!(
        doc.contains(&format!("acdp-registry:{mm}` — a")),
        "the recommended Railway image (`…/acdp-registry:{mm}`) is missing from the \
         'Creating the Railway service' steps"
    );

    // Negative control: the pre-fix page shape is caught.
    assert_eq!(
        railway_version_tags("img `acdp-registry:0.1` or (`:0.1.3`) and `:latest`"),
        vec!["0.1".to_string(), "0.1.3".to_string()]
    );
}

/// `## X.Y.Z` section headings of an UPGRADING page as numeric triples. Only
/// level-2 headings count (`###` does not), trailing text is allowed
/// (`## 0.1.3 and earlier`) and a pre-release suffix is ignored (`0.3.0-rc.1`
/// reads as 0.3.0).
fn upgrading_versions(text: &str) -> Vec<(u64, u64, u64)> {
    let mut out = Vec::new();
    for line in text.lines() {
        let Some(rest) = line.strip_prefix("## ") else {
            continue;
        };
        let head = rest.split(|c: char| c.is_whitespace()).next().unwrap_or("");
        let mut parts = head.splitn(3, '.');
        let (Some(a), Some(b), Some(c)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        let lead = |p: &str| -> Option<u64> {
            let digits: String = p.chars().take_while(char::is_ascii_digit).collect();
            digits.parse().ok()
        };
        if let (Ok(a), Ok(b), Some(c)) = (a.parse::<u64>(), b.parse::<u64>(), lead(c)) {
            out.push((a, b, c));
        }
    }
    out
}

/// The major.minor `docker/RAILWAY.md` must name: the newest of the workspace
/// version and every `## X.Y.Z` heading in the UPGRADING text, compared
/// numerically (0.10 is newer than 0.9).
fn expected_mm(workspace: &str, upgrading_text: &str) -> String {
    let mut best: Option<(u64, u64)> = None;
    let mut parts = workspace.splitn(3, '.');
    if let (Some(a), Some(b)) = (parts.next(), parts.next()) {
        if let (Ok(a), Ok(b)) = (a.parse::<u64>(), b.parse::<u64>()) {
            best = Some((a, b));
        }
    }
    for (a, b, _) in upgrading_versions(upgrading_text) {
        if best.is_none_or(|cur| (a, b) > cur) {
            best = Some((a, b));
        }
    }
    let (a, b) = best.expect("no workspace version or UPGRADING heading to compare");
    format!("{a}.{b}")
}

#[test]
fn expected_mm_follows_the_newest_of_workspace_and_upgrading() {
    // A pending minor: UPGRADING already documents 0.3.0, the workspace is 0.2.0.
    assert_eq!(
        expected_mm("0.2.0", "# Upgrading\n## 0.3.0\nx\n## 0.2.0\ny\n"),
        "0.3"
    );
    // Released: they coincide.
    assert_eq!(expected_mm("0.3.0", "## 0.3.0\n## 0.2.0\n"), "0.3");
    // An older heading never pulls the page backwards.
    assert_eq!(expected_mm("0.3.0", "## 0.2.0\n## 0.1.0\n"), "0.3");
    // Numeric, not lexical: 0.10 is newer than 0.9; the first heading is not special.
    assert_eq!(
        expected_mm("0.9.0", "## 0.9.0\n## 0.10.0\n## 0.2.0\n"),
        "0.10"
    );
    // Only level-2 headings count; trailing text and pre-release suffixes are fine.
    assert_eq!(expected_mm("0.2.0", "### 0.9.0\n## 0.2.0\n"), "0.2");
    assert_eq!(expected_mm("0.2.0", "## 0.3.0-rc.1 (draft)\n"), "0.3");
    assert_eq!(expected_mm("0.2.0", "## 0.1.3 and earlier\n"), "0.2");
    // Non-version headings are ignored.
    assert!(upgrading_versions("## Overview\n## 1.2\n## v0.3.0\n").is_empty());
}

/// Version-shaped image tags: `acdp-registry:<v>` and inline `` `:<v>` ``.
fn railway_version_tags(doc: &str) -> Vec<String> {
    let mut out = Vec::new();
    for marker in ["acdp-registry:", "`:"] {
        for (i, _) in doc.match_indices(marker) {
            let v: String = doc[i + marker.len()..]
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect();
            let v = v.trim_end_matches('.').to_string();
            if v.contains('.') && v.starts_with(|c: char| c.is_ascii_digit()) {
                out.push(v);
            }
        }
    }
    out
}

/// Read `pub const <name>: usize = <n>;` from a source string.
fn usize_const(src: &str, name: &str, origin: &str) -> usize {
    let decl = format!("pub const {name}: usize = ");
    let at = src.find(&decl).unwrap_or_else(|| {
        panic!("`{decl}` not found in {origin}; the guard can no longer read the bound")
    });
    src[at + decl.len()..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .unwrap_or_else(|e| panic!("{name} in {origin} is not a usize literal: {e}"))
}

/// `<a>–<b> <unit>` (en dash or hyphen) occurrences.
fn numeric_ranges(text: &str) -> Vec<(usize, usize, String)> {
    let chars: Vec<char> = text.chars().collect();
    let digits_end = |mut k: usize| {
        while k < chars.len() && chars[k].is_ascii_digit() {
            k += 1;
        }
        k
    };
    let num =
        |a: usize, b: usize| -> usize { chars[a..b].iter().collect::<String>().parse().unwrap() };
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let starts_number =
            chars[i].is_ascii_digit() && (i == 0 || !chars[i - 1].is_ascii_alphanumeric());
        if !starts_number {
            i += 1;
            continue;
        }
        let e1 = digits_end(i);
        let is_range = e1 + 1 < chars.len()
            && (chars[e1] == '–' || chars[e1] == '-')
            && chars[e1 + 1].is_ascii_digit();
        if !is_range {
            i = e1;
            continue;
        }
        let e2 = digits_end(e1 + 1);
        let unit: String = chars[e2..]
            .iter()
            .skip_while(|c| **c == ' ')
            .take_while(|c| c.is_ascii_alphabetic())
            .collect();
        out.push((num(i, e1), num(e1 + 1, e2), unit));
        i = e2;
    }
    out
}

/// The body of the `### <heading>` section of `doc` (up to the next `### `).
fn h3_section<'a>(doc: &'a str, heading: &str, origin: &str) -> &'a str {
    let start = doc
        .find(heading)
        .unwrap_or_else(|| panic!("{origin} lost its {heading} section"));
    let body = &doc[start + heading.len()..];
    &body[..body.find("\n### ").unwrap_or(body.len())]
}

/// D3 guard (c). HTTP-API.md said the challenge `agent_id` "must be a
/// `did:web:` DID (8–2048 bytes)" while the code accepted did:web OR did:key of
/// 9–2048 bytes. The bounds now live in named constants in
/// `acdp-registry-auth`'s `service.rs`; every range stated in the
/// `POST /auth/challenge` section must equal them, in bytes, and the section
/// must name every accepted prefix.
#[test]
fn challenge_did_bounds_in_http_api_match_the_constants() {
    let root = repo_root();
    let origin = "crates/acdp-registry-auth/src/service.rs";
    let src = std::fs::read_to_string(root.join(origin)).expect("read service.rs");
    let min = usize_const(&src, "CHALLENGE_AGENT_ID_MIN_BYTES", origin);
    let max = usize_const(&src, "CHALLENGE_AGENT_ID_MAX_BYTES", origin);
    let pdecl = "pub const CHALLENGE_DID_PREFIXES: [&str; ";
    let p_at = src
        .find(pdecl)
        .unwrap_or_else(|| panic!("CHALLENGE_DID_PREFIXES not declared in {origin}"));
    let p_line = src[p_at..].lines().next().unwrap();
    let prefixes: Vec<&str> = p_line.split('"').skip(1).step_by(2).collect();
    assert!(
        prefixes.len() >= 2,
        "parsed prefixes {prefixes:?} from {p_line:?}"
    );

    let doc = std::fs::read_to_string(root.join("docs/HTTP-API.md")).expect("read HTTP-API.md");
    let section = h3_section(&doc, "### `POST /auth/challenge`", "docs/HTTP-API.md");

    let ranges = numeric_ranges(section);
    assert!(
        ranges.len() >= 2,
        "the challenge section states the length bound {} time(s); expected both the \
         screen paragraph and the summary line to state it: {ranges:?}",
        ranges.len()
    );
    for (lo, hi, unit) in &ranges {
        assert!(
            (*lo, *hi) == (min, max) && unit == "bytes",
            "docs/HTTP-API.md's challenge section says {lo}–{hi} {unit}; the code \
             accepts {min}–{max} bytes (CHALLENGE_AGENT_ID_MIN_BYTES/MAX_BYTES)"
        );
    }
    for p in &prefixes {
        assert!(
            section.contains(&format!("`{p}`")),
            "the challenge section never names the accepted prefix `{p}`"
        );
    }
    assert!(
        !section.contains("must be a `did:web:` DID ("),
        "the challenge section restricts agent_id to did:web again, but \
         {prefixes:?} are all accepted"
    );

    // Negative control: the pre-fix sentences are caught.
    assert_eq!(
        numeric_ranges("a `did:web:` DID (8–2048 bytes). and 9–2048 characters"),
        vec![(8, 2048, "bytes".into()), (9, 2048, "characters".into())]
    );
}

/// Blockquote paragraphs (consecutive `>` lines, markers stripped, joined).
fn blockquotes(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur: Option<String> = None;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix('>') {
            let c = cur.get_or_insert_with(String::new);
            c.push(' ');
            c.push_str(rest.trim());
        } else if let Some(c) = cur.take() {
            out.push(c);
        }
    }
    out.extend(cur);
    out
}

/// D3 guard (d). CONFIGURATION.md's `[rate_limit]` caveat said the
/// `global_per_minute` ceiling "is per replica" unconditionally — false since
/// `backend = "postgres"` counts it once across replicas. Any caveat
/// blockquote in that section that talks about per-replica counting must name
/// both backends, so the claim is scoped to the one it is true for.
#[test]
fn configuration_rate_limit_caveat_names_the_backend() {
    let root = repo_root();
    let doc =
        std::fs::read_to_string(root.join("docs/CONFIGURATION.md")).expect("read CONFIGURATION.md");
    let section = h3_section(&doc, "### `[rate_limit]`", "docs/CONFIGURATION.md");

    let quotes = blockquotes(section);
    let per_replica: Vec<&String> = quotes
        .iter()
        .filter(|q| q.contains("per replica"))
        .collect();
    assert!(
        !per_replica.is_empty(),
        "the [rate_limit] section no longer carries its multi-replica caveat \
         blockquote; restore it (scoped by backend) or retire this guard"
    );
    for q in per_replica {
        assert!(
            q.contains("backend = \"memory\"") && q.contains("backend = \"postgres\""),
            "the [rate_limit] caveat states per-replica limits without naming the \
             `backend` it applies to — under `backend = \"postgres\"` both ceilings \
             are shared across replicas:\n{q}"
        );
    }
}

// ---- docs-refresh Phase 0: relative doc links resolve ----------------------

/// Tracked markdown the relative-link guard does NOT read, each with its reason.
/// A trailing `/` is a directory prefix; one `*` matches exactly one path
/// segment. Every entry must still match a tracked file (asserted), so an
/// exclusion cannot outlive what it excludes and quietly cover something new.
const LINK_GUARD_EXCLUDED: [(&str, &str); 5] = [
    (
        "plans/",
        "planning notes: gitignored apart from a few tracked cross-repo notes, \
         never published, and they cite gitignored plan files by path",
    ),
    (
        "crates/*/CHANGELOG.md",
        "generated by release-plz from commit subjects; its links are the \
         absolute compare/PR URLs it writes itself",
    ),
    (
        "DECISIONS.md",
        "append-only workflow ledger: its entries are never edited after the \
         fact and quote link-shaped text (paths, gitignored plans/ files, \
         examples of the very links a guard rejects)",
    ),
    (
        "ASSUMPTIONS.md",
        "append-only workflow ledger: its entries are never edited after the \
         fact and quote link-shaped text (paths, gitignored plans/ files, \
         examples of the very links a guard rejects)",
    ),
    (
        "docs/ENGINEERING-LOG.md",
        "a dated record that is never edited after the fact; it carries 4 \
         relative links written against an older layout that no longer resolve",
    ),
];

/// True when tracked `path` is covered by one `LINK_GUARD_EXCLUDED` pattern.
fn link_guard_excludes(path: &str, pattern: &str) -> bool {
    if let Some(dir) = pattern.strip_suffix('/') {
        return path.starts_with(&format!("{dir}/"));
    }
    match pattern.split_once('*') {
        Some((pre, suf)) => {
            path.len() >= pre.len() + suf.len()
                && path.starts_with(pre)
                && path.ends_with(suf)
                && !path[pre.len()..path.len() - suf.len()].contains('/')
        }
        None => path == pattern,
    }
}

/// The fence a line could open or close: its character and run length, when
/// its first non-blank characters are three or more backticks or tildes.
fn fence_marker(line: &str) -> Option<(char, usize)> {
    let t = line.trim_start();
    let c = t.chars().next()?;
    if c != '`' && c != '~' {
        return None;
    }
    let n = t.chars().take_while(|&x| x == c).count();
    (n >= 3).then_some((c, n))
}

/// The lines of `text` outside fenced code blocks, numbered from 1. A fence
/// closes only on a run of the SAME character at least as long with nothing
/// after it, so a "```text" line inside a "~~~" block does not end the block.
fn unfenced_lines(text: &str) -> Vec<(usize, &str)> {
    let mut open: Option<(char, usize)> = None;
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        match (open, fence_marker(line)) {
            (None, Some(m)) => open = Some(m),
            (Some((c, n)), Some((c2, n2)))
                if c == c2 && n2 >= n && line.trim().chars().all(|x| x == c) =>
            {
                open = None
            }
            (Some(_), _) => {}
            (None, None) => out.push((i + 1, line)),
        }
    }
    out
}

/// `line` with every inline code span replaced by a space. A span opens on a
/// run of N backticks and closes on the next run of exactly N; a run with no
/// partner is literal text, as in CommonMark.
fn strip_code_spans(line: &str) -> String {
    let b = line.as_bytes();
    let run_at = |j: usize| b[j..].iter().take_while(|&&x| x == b'`').count();
    let mut out = String::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'`' {
            let ch = line[i..].chars().next().expect("i is on a char boundary");
            out.push(ch);
            i += ch.len_utf8();
            continue;
        }
        let n = run_at(i);
        let mut j = i + n;
        let mut close = None;
        while j < b.len() {
            if b[j] == b'`' {
                let m = run_at(j);
                if m == n {
                    close = Some(j);
                    break;
                }
                j += m;
            } else {
                j += 1;
            }
        }
        match close {
            Some(j) => {
                out.push(' ');
                i = j + n;
            }
            None => {
                out.push_str(&line[i..i + n]);
                i += n;
            }
        }
    }
    out
}

/// The link destination at the start of `s`: the `<...>` contents when
/// bracketed, else everything up to the first whitespace or `)`.
fn link_destination(s: &str) -> Option<String> {
    let d = match s.strip_prefix('<') {
        Some(rest) => rest.split('>').next().unwrap_or(""),
        None => s
            .split(|c: char| c.is_whitespace() || c == ')')
            .next()
            .unwrap_or(""),
    };
    (!d.is_empty()).then(|| d.to_string())
}

/// Every non-external link target in markdown `text`, as (1-based line,
/// target): inline links and images (`[t](u)`, `![a](u)`) and reference
/// definitions (`[label]: u`). Fenced blocks and inline code are skipped, since
/// a link-shaped example there is not a link; `http:`, `https:` and `mailto:`
/// targets are dropped. A same-file `#fragment` IS returned -- it is checked
/// against the file's own headings, which a renamed heading breaks just as
/// surely as it breaks a cross-file link.
fn relative_links(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for (n, raw) in unfenced_lines(text) {
        let line = strip_code_spans(raw);
        let mut targets: Vec<Option<String>> =
            line.split("](").skip(1).map(link_destination).collect();
        let t = line.trim_start();
        if t.starts_with('[') && !t.starts_with("[^") && line.len() - t.len() <= 3 {
            if let Some(close) = t.find("]:") {
                if !t[1..close].contains(']') {
                    targets.push(link_destination(t[close + 2..].trim_start()));
                }
            }
        }
        for target in targets.into_iter().flatten() {
            let lower = target.to_ascii_lowercase();
            if ["http://", "https://", "mailto:"]
                .iter()
                .any(|s| lower.starts_with(s))
            {
                continue;
            }
            out.push((n, target));
        }
    }
    out
}

/// GitHub's anchor for a heading: link destinations dropped (the RENDERED text
/// is what gets slugged), lowercased, every character that is not a letter,
/// digit, `_`, `-` or space removed -- backticks and other punctuation included
/// -- and each space turned into `-`. So `A & B — C` becomes `a--b--c`: the `&`
/// and the em dash vanish and the spaces on either side of them remain.
fn github_slug(heading: &str) -> String {
    let mut text = String::new();
    let mut rest = heading;
    while let Some(i) = rest.find("](") {
        text.push_str(&rest[..i]);
        rest = match rest[i..].find(')') {
            Some(j) => &rest[i + j + 1..],
            None => "",
        };
    }
    text.push_str(rest);
    text.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | ' '))
        .map(|c| if c == ' ' { '-' } else { c })
        .collect()
}

/// The anchors GitHub generates for `text`'s ATX headings, in document order.
/// A repeated slug gets `-1`, `-2`, …; a `#` line inside a fenced block is not
/// a heading. An optional closing `#` sequence is not part of the text.
fn heading_anchors(text: &str) -> Vec<String> {
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut out = Vec::new();
    for (_, line) in unfenced_lines(text) {
        let hashes = line.chars().take_while(|&c| c == '#').count();
        if !(1..=6).contains(&hashes) {
            continue;
        }
        let rest = &line[hashes..];
        if !(rest.is_empty() || rest.starts_with([' ', '\t'])) {
            continue;
        }
        let mut content = rest.trim();
        let unclosed = content.trim_end_matches('#');
        if unclosed.is_empty() || unclosed.ends_with([' ', '\t']) {
            content = unclosed.trim_end();
        }
        let slug = github_slug(content);
        let k = seen.entry(slug.clone()).or_insert(0);
        out.push(if *k == 0 { slug } else { format!("{slug}-{k}") });
        *k += 1;
    }
    out
}

/// `%XX` escapes decoded; anything else, including a malformed escape, kept.
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Some(v) = std::str::from_utf8(&b[i + 1..i + 3])
                .ok()
                .and_then(|h| u8::from_str_radix(h, 16).ok())
            {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `target`, relative to the directory of repo-relative `from`, as a
/// repo-relative path (a leading `/` is the repository root, as GitHub renders
/// it -- though `relative_link_violations` rejects such links before resolving
/// them); `None` if it climbs above the root.
fn resolve_relative(from: &str, target: &str) -> Option<String> {
    let mut parts: Vec<&str> = from.split('/').collect();
    parts.pop();
    let target = match target.strip_prefix('/') {
        Some(abs) => {
            parts.clear();
            abs
        }
        None => target,
    };
    for seg in target.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            s => parts.push(s),
        }
    }
    Some(parts.join("/"))
}

/// Every broken relative link in the markdown file `source` (repo-relative)
/// whose text is `text`. A target must be a git-TRACKED file or directory --
/// a file that exists only on the author's disk renders as a 404 on GitHub and
/// on the website -- and a `#fragment` on a `.md` target must equal one of that
/// file's GitHub heading anchors. `read` yields a tracked target's text. Pure
/// over its inputs, so the negative controls drive it with synthetic files.
fn relative_link_violations(
    source: &str,
    text: &str,
    tracked: &std::collections::BTreeSet<String>,
    read: &dyn Fn(&str) -> Option<String>,
) -> Vec<String> {
    let mut out = Vec::new();
    for (line, target) in relative_links(text) {
        // GitHub resolves a leading `/` against the repository root, but the
        // website serves it from the SITE root, where it 404s.
        if target.starts_with('/') {
            out.push(format!(
                "{source} line {line}: `{target}` starts with `/`; the website serves \
                 it from the site root (a 404) -- write it relative to this file"
            ));
            continue;
        }
        let (path, frag) = match target.split_once('#') {
            Some((p, f)) => (p, Some(f)),
            None => (target.as_str(), None),
        };
        let path = percent_decode(path);
        let resolved = if path.is_empty() {
            Some(source.to_string())
        } else {
            resolve_relative(source, &path)
        };
        let Some(resolved) = resolved else {
            out.push(format!(
                "{source} line {line}: `{target}` climbs above the repository root"
            ));
            continue;
        };
        let is_file = tracked.contains(&resolved);
        let dir_prefix = format!("{resolved}/");
        let is_dir = resolved.is_empty() || tracked.iter().any(|t| t.starts_with(&dir_prefix));
        if !is_file && !is_dir {
            out.push(format!(
                "{source} line {line}: `{target}` -> `{resolved}` is not a tracked file \
                 or directory"
            ));
            continue;
        }
        let Some(frag) = frag.filter(|f| !f.is_empty()) else {
            continue;
        };
        if !(is_file && resolved.ends_with(".md")) {
            continue;
        }
        let Some(target_text) = (if resolved == source {
            Some(text.to_string())
        } else {
            read(&resolved)
        }) else {
            out.push(format!(
                "{source} line {line}: `{target}` -> `{resolved}` is tracked but unreadable"
            ));
            continue;
        };
        let frag = percent_decode(frag);
        if !heading_anchors(&target_text).contains(&frag) {
            out.push(format!(
                "{source} line {line}: `{target}` -> `{resolved}` has no heading whose \
                 GitHub anchor is `#{frag}`"
            ));
        }
    }
    out
}

/// No relative link in tracked documentation is dead. Before this guard,
/// nothing checked them: four links in `docs/ENGINEERING-LOG.md` had been dead
/// since files moved, and a renamed heading silently orphans every
/// `FILE.md#anchor` pointing at it -- the docs index links into HTTP-API.md
/// sections by anchor.
///
/// Scope: every git-tracked `*.md` minus `LINK_GUARD_EXCLUDED` (each entry with
/// its reason). Stated limits: no network (external URLs are another guard's
/// business); a fragment on a non-markdown target (`file.rs#L10`) is not
/// checked; explicit HTML anchors (`<a id>`) are not recognised as targets,
/// so using one fails here loudly rather than passing unchecked; the slug rules
/// are GitHub's ASCII-and-Unicode-letter rules, not every edge of its renderer
/// (HTML inside a heading, for one).
#[test]
fn every_relative_doc_link_resolves() {
    let root = repo_root();
    let tracked: std::collections::BTreeSet<String> =
        git_tracked_paths(&root).into_iter().collect();

    for (pattern, reason) in LINK_GUARD_EXCLUDED {
        assert!(
            tracked.iter().any(|p| link_guard_excludes(p, pattern)),
            "LINK_GUARD_EXCLUDED entry `{pattern}` ({reason}) matches no tracked file: \
             drop it, so it cannot later exclude something new by accident"
        );
    }
    let excluded = |p: &str| {
        LINK_GUARD_EXCLUDED
            .iter()
            .any(|(pattern, _)| link_guard_excludes(p, pattern))
    };
    let scope: Vec<&String> = tracked
        .iter()
        .filter(|p| p.ends_with(".md") && !excluded(p.as_str()))
        .collect();

    // Vacuity guard by NAMED members, not a count: the scope must hold the files
    // operators read, and the extractor must find two specific real links, one
    // with an anchor -- a broken enumeration or extractor fails here instead of
    // passing everything.
    for must in [
        "README.md",
        "CHANGELOG.md",
        "CONTRIBUTING.md",
        "docs/README.md",
        "docs/HTTP-API.md",
        "docker/RAILWAY.md",
    ] {
        assert!(
            scope.iter().any(|p| p.as_str() == must),
            "relative-link scope lost {must}: {scope:?}"
        );
    }
    assert!(
        !scope
            .iter()
            .any(|p| p.as_str() == "docs/ENGINEERING-LOG.md"),
        "the dated engineering log must stay out of scope"
    );

    let read = |p: &str| std::fs::read_to_string(root.join(p)).ok();
    let mut links: Vec<(String, String)> = Vec::new();
    let mut broken: Vec<String> = Vec::new();
    for source in &scope {
        let text =
            read(source).unwrap_or_else(|| panic!("{source} is tracked but not readable on disk"));
        links.extend(
            relative_links(&text)
                .into_iter()
                .map(|(_, t)| (source.to_string(), t)),
        );
        broken.extend(relative_link_violations(source, &text, &tracked, &read));
    }
    for (source, target) in [
        ("docs/README.md", "HTTP-API.md#error-envelope"),
        ("CHANGELOG.md", "docs/UPGRADING.md"),
    ] {
        assert!(
            links.iter().any(|(s, t)| s == source && t == target),
            "the extractor no longer finds the link {source} -> {target}; either it \
             broke or the link moved (then name another real one here)"
        );
    }

    assert!(
        broken.is_empty(),
        "dead relative links in tracked documentation:\n  {}\n\
         Point each at a tracked file and an existing heading. Anchors follow \
         GitHub's slug: lowercase, punctuation (including `&`, `.`, `/`, backticks \
         and em dashes) dropped, spaces become `-`, repeats get `-1`, `-2`.",
        broken.join("\n  ")
    );
}

/// The link guard's matcher must reject each kind of dead link and must ignore
/// what is not a link -- on synthetic files, so every case is attributable.
#[test]
fn the_relative_link_guard_rejects_what_it_must_and_ignores_what_it_should() {
    let files: std::collections::HashMap<&str, &str> = [
        (
            "docs/b.md",
            "# Intro\n\
             ## Setup\n\
             ## Setup\n\
             ## A & B — C\n\
             ```\n\
             # Not A Heading\n\
             ```\n\
             ## `code` and [link](z.md) heading ##\n\
             ### GET /contexts/{ctx_id}\n\
             ## Café\n",
        ),
        ("README.md", "# Root\n"),
        ("untracked.md", "# On disk only\n"),
    ]
    .into_iter()
    .collect();
    let tracked: std::collections::BTreeSet<String> =
        ["docs/a.md", "docs/b.md", "README.md", "img/x.png"]
            .into_iter()
            .map(str::to_string)
            .collect();
    let read = |p: &str| files.get(p).map(|s| s.to_string());
    let check = |text: &str| relative_link_violations("docs/a.md", text, &tracked, &read);

    // Every one of these resolves.
    let good = "# Here\n\
                [b](b.md) and [b](b.md#setup) and [dup](b.md#setup-1)\n\
                [amp and dash](b.md#a--b--c) [code](b.md#code-and-link-heading)\n\
                [route](b.md#get-contextsctx_id) [pct](b.md#caf%C3%A9)\n\
                [root](../README.md) ![img](../img/x.png) [dir](../img/) [self](#here)\n\
                [ext](https://example.com/nope.md) [mail](mailto:a@example.com)\n\
                [ref]: b.md#intro\n\
                [^note]: not a link target\n\
                not a link: `[x](missing.md)` nor ``[y](`missing.md`)``\n\
                ~~~\n\
                [fenced](missing.md)\n\
                ```text\n\
                [still fenced](missing.md)\n\
                ~~~\n";
    assert_eq!(
        check(good),
        Vec::<String>::new(),
        "valid links, code spans and fenced examples must all pass"
    );
    assert_eq!(
        relative_links(good).len(),
        12,
        "the extractor must see all 12 real non-external links above and nothing \
         inside code: {:?}",
        relative_links(good)
    );

    // Each of these is dead in exactly one way.
    for (label, bad) in [
        ("missing file", "[x](nope.md)"),
        ("missing anchor", "[x](b.md#nope)"),
        (
            "untracked target that exists on disk",
            "[x](../untracked.md)",
        ),
        (
            "anchor of a fenced pseudo-heading",
            "[x](b.md#not-a-heading)",
        ),
        ("third duplicate that does not exist", "[x](b.md#setup-2)"),
        ("self anchor that does not exist", "[x](#nope)"),
        ("climbs above the root", "[x](../../outside.md)"),
        ("reference definition to a missing file", "[r]: gone.md"),
        ("image that is not tracked", "![i](../img/y.png)"),
        // Resolves on GitHub (repo root) -- rejected for the website alone.
        ("leading `/` to a tracked file", "[x](/README.md)"),
        ("leading `/` reference definition", "[r]: /docs/b.md#intro"),
    ] {
        let found = check(bad);
        assert_eq!(
            found.len(),
            1,
            "{label}: `{bad}` must produce exactly one violation, got {found:?}"
        );
    }

    // The slug rules themselves, against the cases the plan names.
    assert_eq!(github_slug("A & B — C"), "a--b--c");
    assert_eq!(github_slug("`POST /auth/challenge`"), "post-authchallenge");
    assert_eq!(github_slug("Error envelope"), "error-envelope");
    assert_eq!(
        heading_anchors("# X\n## X\n## X ##\n#NoSpace\n"),
        vec!["x", "x-1", "x-2"]
    );
    assert!(link_guard_excludes(
        "crates/acdp-registry-core/CHANGELOG.md",
        "crates/*/CHANGELOG.md"
    ));
    assert!(!link_guard_excludes(
        "crates/a/b/CHANGELOG.md",
        "crates/*/CHANGELOG.md"
    ));
    assert!(link_guard_excludes("plans/cross-repo/x.md", "plans/"));
    assert!(!link_guard_excludes("plansx.md", "plans/"));
}

// ---- docs-refresh Phase 6: sibling-repository links stay pinned -------------
//
// A link to a sibling repository's `main` silently changes meaning whenever that
// repository moves: before this guard every spec and SDK link here pointed at
// `main`, and the SDK guides on `main` had already drifted from the version this
// registry builds against. `docs/README.md`'s "Link convention" block is the
// human-readable form of these rules; this is the enforced form.
//
// Like every test in this file it also runs in cargo-mutants' baseline
// (`.cargo/mutants.toml` sets `copy_vcs = true`, so `git ls-files` works in the
// copy): a failure here fails that baseline too, not only the PR jobs.

/// The GitHub organisation every sibling repository lives in.
const ACDP_ORG: &str = "agentcontextdistributionprotocol";
/// This repository: its links may use `main`, the revision these docs describe
/// (the website copies absolute URLs verbatim; it rewrites only relative links).
const THIS_REPO: &str = "acdp-registry-rs";
/// The spec repository. Its links must use the `.spec-pin` `ref:` value.
const SPEC_REPO: &str = "agentcontextdistributionprotocol";
/// The SDK repository. Its links must use an `acdp-v<semver>` tag or a full SHA.
const SDK_REPO: &str = "acdp-rs";
/// The ONE spec ref allowed besides the pin, and only on the spec's
/// non-normative `docs/` pages: `fb76f6d`, the spec docs refresh that followed
/// the pinned revision (see the Link-convention block in `docs/README.md`).
/// Normative paths (`rfcs/`, `registries/`, `schemas/`, ...) must use the pin.
const SPEC_DOCS_REF: &str = "fb76f6d54ba25f583ce526b2bdee30503a5d8e59";

/// Where one sibling URL points.
#[derive(Debug, Clone, PartialEq)]
enum SiblingTarget {
    /// A file or directory at a ref: `github.com/<org>/<repo>/(blob|tree|raw)/<ref>/<path>`
    /// or `raw.githubusercontent.com/<org>/<repo>/<ref>/<path>`. A
    /// `refs/heads/<name>` ref is kept whole.
    AtRef {
        repo: String,
        git_ref: String,
        path: String,
    },
    /// Any other URL into an org repository (the repo root, issues, releases):
    /// nothing to pin.
    Unpinned { repo: String },
    /// `docs.rs/<acdp or acdp-*>/<version?>/...` or
    /// `docs.rs/crate/<acdp or acdp-*>/<version?>/...`; an `_` in the crate name
    /// is read as `-` (docs.rs serves both spellings).
    DocsRs {
        krate: String,
        version: Option<String>,
    },
}

/// How a URL is written in markdown. Sibling links must be `Inline`: a
/// consistency rule (one form to scan and re-point), not a website workaround --
/// the website copies absolute URLs verbatim whatever their form.
#[derive(Debug, Clone, Copy, PartialEq)]
enum LinkForm {
    /// `[text](url)`.
    Inline,
    /// `[label]: url`.
    ReferenceDefinition,
    /// Anything else: a bare URL, an autolink, an HTML attribute.
    Bare,
}

#[derive(Debug, Clone)]
struct SiblingLink {
    line: usize,
    url: String,
    target: SiblingTarget,
    form: LinkForm,
}

/// `X.Y.Z` with all three parts non-empty decimal digits.
fn is_core_semver(v: &str) -> bool {
    let parts: Vec<&str> = v.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|c| c.is_ascii_digit()))
}

/// A concrete crate version as docs.rs serves it: `X.Y.Z` with an optional
/// `-prerelease`. `latest`, `*`, `^0.14`, `~0.14` and a missing segment are not.
fn is_crate_version(v: &str) -> bool {
    let (core, pre) = match v.split_once('-') {
        Some((c, p)) => (c, Some(p)),
        None => (v, None),
    };
    is_core_semver(core)
        && pre.is_none_or(|p| {
            !p.is_empty()
                && p.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        })
}

/// A ref the SDK repository may be linked at: a release tag `acdp-v<X.Y.Z>`
/// or a full 40-hex commit SHA. Not `main`, not a short SHA.
fn is_sdk_pin(r: &str) -> bool {
    is_sha40(r) || r.strip_prefix("acdp-v").is_some_and(is_core_semver)
}

/// Splits `segs` (the path after `<org>/<repo>/[kind/]`) into (ref, path). A
/// `refs/<heads|tags>/<name>` ref spans three segments.
fn split_ref(segs: &[&str]) -> (String, String) {
    let n = if segs.first() == Some(&"refs") { 3 } else { 1 };
    let n = n.min(segs.len());
    (segs[..n].join("/"), segs[n..].join("/"))
}

/// Classifies one URL, or `None` when it is neither a URL into an org
/// repository nor an `acdp*` docs.rs URL.
fn classify_sibling_url(url: &str) -> Option<SiblingTarget> {
    let lower = url.to_ascii_lowercase();
    let mut bare = url;
    for prefix in ["https://", "http://", "www."] {
        if lower[url.len() - bare.len()..].starts_with(prefix) {
            bare = &bare[prefix.len()..];
        }
    }
    // The fragment and query are not part of the ref or path.
    let bare = bare.split(['#', '?']).next().unwrap_or("");
    let segs: Vec<&str> = bare.split('/').collect();
    let host = segs.first()?.to_ascii_lowercase();
    match host.as_str() {
        "github.com" | "raw.githubusercontent.com" => {
            if !segs.get(1)?.eq_ignore_ascii_case(ACDP_ORG) {
                return None;
            }
            let repo = segs.get(2).filter(|r| !r.is_empty())?.to_ascii_lowercase();
            let rest = &segs[3.min(segs.len())..];
            if host == "raw.githubusercontent.com" {
                let (git_ref, path) = split_ref(rest);
                return Some(SiblingTarget::AtRef {
                    repo,
                    git_ref,
                    path,
                });
            }
            // GitHub matches the kind segment case-insensitively (`/BLOB/main/`
            // serves the same file), so the guard must too.
            match rest.first().map(|k| k.to_ascii_lowercase()).as_deref() {
                Some("blob" | "tree" | "raw") => {
                    let (git_ref, path) = split_ref(&rest[1..]);
                    Some(SiblingTarget::AtRef {
                        repo,
                        git_ref,
                        path,
                    })
                }
                _ => Some(SiblingTarget::Unpinned { repo }),
            }
        }
        "docs.rs" => {
            // `docs.rs/crate/<name>/<version>` is the crate-info form of
            // `docs.rs/<name>/<version>`; both follow `latest` without a version.
            let at = if segs.get(1).is_some_and(|s| s.eq_ignore_ascii_case("crate")) {
                2
            } else {
                1
            };
            let krate = segs.get(at)?.to_ascii_lowercase().replace('_', "-");
            if !(krate == "acdp" || krate.starts_with("acdp-")) {
                return None;
            }
            let version = segs
                .get(at + 1)
                .filter(|v| !v.is_empty())
                .map(|v| v.to_string());
            Some(SiblingTarget::DocsRs { krate, version })
        }
        _ => None,
    }
}

/// Every sibling-repository or `acdp*` docs.rs URL in `text`, with its line and
/// form. In markdown, fenced blocks and inline code are skipped (a link-shaped
/// example there is not a link -- the Link-convention block itself spells
/// `blob/<ref>/` templates in code spans). Other files (`config/*.toml`,
/// `docker/*`) are scanned line by line as they are: a URL in a comment is
/// still read by someone.
fn sibling_links(text: &str, markdown: bool) -> Vec<SiblingLink> {
    let lines: Vec<(usize, String)> = if markdown {
        unfenced_lines(text)
            .into_iter()
            .map(|(n, l)| (n, strip_code_spans(l)))
            .collect()
    } else {
        text.lines()
            .enumerate()
            .map(|(i, l)| (i + 1, l.to_string()))
            .collect()
    };
    let hosts = ["github.com/", "raw.githubusercontent.com/", "docs.rs/"];
    let mut out = Vec::new();
    for (n, line) in lines {
        // ASCII lowercasing keeps byte offsets, so positions found in `lower`
        // index `line` too.
        let lower = line.to_ascii_lowercase();
        // (start, host start, has scheme) of every URL whose host is one of `hosts`.
        // A URL needs its `http(s)://` scheme: link TEXT such as
        // `[docs.rs/acdp 0.14.3](...)` names a host without being a link, and
        // the scheme also anchors the host, so `gist.github.com/` is not
        // mistaken for `github.com/`.
        // The one scheme-less exception: GitHub autolinks `www.<host>/...`, so a
        // `www.` prefix (with or without a scheme) is a URL too.
        let host_at = |at: usize| {
            let h = lower[at..].strip_prefix("www.").unwrap_or(&lower[at..]);
            hosts.iter().any(|x| h.starts_with(x))
        };
        let mut starts: Vec<(usize, usize, bool)> = Vec::new();
        for scheme in ["https://", "http://"] {
            let mut from = 0;
            while let Some(i) = lower[from..].find(scheme) {
                let start = from + i;
                let at = start + scheme.len();
                if host_at(at) {
                    starts.push((start, at, true));
                }
                from = at;
            }
        }
        let mut from = 0;
        while let Some(i) = lower[from..].find("www.") {
            let start = from + i;
            // Not after `://` (seen above) nor inside a longer host name.
            let glued = lower[..start]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '/'));
            if !glued && host_at(start) {
                starts.push((start, start, false));
            }
            from = start + 4;
        }
        starts.sort_unstable();
        for (start, at, has_scheme) in starts {
            let end = line[at..]
                .find(|c: char| {
                    c.is_whitespace()
                        || matches!(
                            c,
                            ')' | '(' | '<' | '>' | '"' | '\'' | '`' | ']' | '[' | '|'
                        )
                })
                .map_or(line.len(), |e| at + e);
            let url = line[at..end].trim_end_matches(['.', ',', ';', ':']);
            let Some(target) = classify_sibling_url(url) else {
                continue;
            };
            let end = at + url.len();
            // Scheme-less text that is itself a link's TEXT (`[www.github.com/x](u)`)
            // is not a URL; the destination `u` is scanned on its own.
            if !has_scheme && line[end..].starts_with("](") {
                continue;
            }
            let before = &line[..start];
            let def_prefix = before.trim_end().trim_end_matches('<').trim_end();
            let def_line = def_prefix.trim_start();
            let form = if before.ends_with("](") || before.ends_with("](<") {
                LinkForm::Inline
            } else if def_prefix.ends_with("]:")
                && def_line.starts_with('[')
                && !def_line.starts_with("[^")
            {
                LinkForm::ReferenceDefinition
            } else {
                LinkForm::Bare
            };
            out.push(SiblingLink {
                line: n,
                url: line[start..end].to_string(),
                target,
                form,
            });
        }
    }
    out
}

/// Why one sibling link breaks the pinning rules, or `None` when it is fine.
/// `spec_ref` is the `ref:` value of `.spec-pin`.
fn sibling_link_problem(link: &SiblingLink, markdown: bool, spec_ref: &str) -> Option<String> {
    // `HEAD` is the default branch by another name: it moves just like `main`.
    let is_branch = |r: &str| {
        let last = r.rsplit('/').next().unwrap_or(r).to_ascii_lowercase();
        matches!(last.as_str(), "main" | "master" | "head")
    };
    let repo = match &link.target {
        SiblingTarget::AtRef { repo, .. } | SiblingTarget::Unpinned { repo } => Some(repo),
        SiblingTarget::DocsRs { .. } => None,
    };
    if repo.is_some_and(|r| r == THIS_REPO) {
        return None;
    }
    if markdown && repo.is_some() && link.form != LinkForm::Inline {
        return Some(if link.form == LinkForm::ReferenceDefinition {
            "is a reference-style definition; sibling links must be inline `[text](url)` \
             (a consistency rule: one form to scan and re-point on a bump)"
                .to_string()
        } else {
            "is not an inline `[text](url)` link (bare URL, autolink or HTML); sibling \
             links are inline only, so every pinned link has one form to scan and \
             re-point on a bump"
                .to_string()
        });
    }
    match &link.target {
        SiblingTarget::Unpinned { .. } => None,
        SiblingTarget::DocsRs { krate, version } => match version {
            Some(v) if is_crate_version(v) => None,
            Some(v) => Some(format!(
                "docs.rs/{krate} at `{v}` is not a concrete version; write the version \
                 `Cargo.lock` resolves (e.g. docs.rs/{krate}/0.14.3/...)"
            )),
            None => Some(format!(
                "docs.rs/{krate} has no version segment, so it follows whatever is \
                 latest; write the version `Cargo.lock` resolves"
            )),
        },
        SiblingTarget::AtRef {
            repo,
            git_ref,
            path,
        } => {
            if repo == SPEC_REPO {
                if git_ref == spec_ref || (git_ref == SPEC_DOCS_REF && path.starts_with("docs/")) {
                    None
                } else if git_ref == SPEC_DOCS_REF {
                    Some(format!(
                        "uses SPEC_DOCS_REF on `{path}`, which is not under the spec's \
                         `docs/`; normative spec paths must use the `.spec-pin` ref \
                         `{spec_ref}`"
                    ))
                } else {
                    Some(format!(
                        "links the spec at `{git_ref}`; spec links must use exactly the \
                         `.spec-pin` ref `{spec_ref}` (no branch, tag or short SHA) -- or \
                         SPEC_DOCS_REF on a `docs/` page"
                    ))
                }
            } else if repo == SDK_REPO {
                (!is_sdk_pin(git_ref)).then(|| {
                    format!(
                        "links acdp-rs at `{git_ref}`; use a release tag `acdp-v<X.Y.Z>` or \
                         a full 40-hex SHA"
                    )
                })
            } else if git_ref.is_empty() || is_branch(git_ref) {
                Some(format!(
                    "links sibling `{repo}` at `{git_ref}`; pin a tag or commit SHA, never \
                     `main`/`master`"
                ))
            } else {
                None
            }
        }
    }
}

/// Every pinning violation in `source` (repo-relative) whose text is `text`.
/// Pure over its inputs, so the negative controls drive it with synthetic text.
fn sibling_link_violations(source: &str, text: &str, spec_ref: &str) -> Vec<String> {
    let markdown = source.ends_with(".md");
    sibling_links(text, markdown)
        .into_iter()
        .filter_map(|l| {
            sibling_link_problem(&l, markdown, spec_ref)
                .map(|why| format!("{source} line {}: `{}` {why}", l.line, l.url))
        })
        .collect()
}

/// The `.spec-pin` `ref:` and `repository:` values, parsed the way
/// `spec_pin_violations` reads the file (column-0 declarations, exactly one each).
fn spec_pin_ref_and_repo(spec_pin: &str) -> (String, String) {
    let value = |key: &str, pred: fn(&str) -> bool| {
        let lines = pin_declarations(spec_pin, key, pred);
        assert_eq!(
            lines.len(),
            1,
            ".spec-pin must declare `{key}:` exactly once (see spec_pin_violations)"
        );
        spec_pin
            .lines()
            .nth(lines[0] - 1)
            .and_then(|l| l.strip_prefix(key))
            .and_then(|r| r.strip_prefix(": "))
            .expect("pin_declarations matched this line")
            .to_string()
    };
    (value("ref", is_sha40), value("repository", is_owner_repo))
}

/// No tracked document links a sibling repository at a moving ref.
///
/// Scope: every git-tracked `*.md` minus `LINK_GUARD_EXCLUDED` (its `plans/`
/// entry also covers the tracked `plans/cross-repo/` notes, its
/// `crates/*/CHANGELOG.md` entry the generated changelogs), plus tracked
/// `config/*.toml` and `docker/*` text files. Rules (`docs/README.md`, "Link
/// convention"): links to THIS repository are free; the spec must be at the
/// `.spec-pin` ref (or SPEC_DOCS_REF on `docs/` pages); acdp-rs at an
/// `acdp-v<X.Y.Z>` tag or a full SHA; any other sibling at anything but
/// `main`/`master`/`HEAD`; docs.rs `acdp*` links carry a concrete version; and every
/// sibling URL in markdown is an inline link.
///
/// A `.spec-pin` bump therefore fails this test until the pinned spec links are
/// re-pointed -- intended; `docs/MAINTAINING.md` ("Spec bumps") gives the
/// one-line rewrite. No network: an anchor missing at the pinned revision is not
/// detected here.
#[test]
fn sibling_repo_links_are_pinned() {
    let root = repo_root();
    let tracked = git_tracked_paths(&root);
    let spec_pin = std::fs::read_to_string(root.join(".spec-pin")).expect("read .spec-pin");
    let (spec_ref, spec_repo) = spec_pin_ref_and_repo(&spec_pin);
    assert_eq!(
        spec_repo,
        format!("{ACDP_ORG}/{SPEC_REPO}"),
        ".spec-pin names a different spec repository than this guard checks"
    );
    assert!(is_sha40(SPEC_DOCS_REF), "SPEC_DOCS_REF must be a full SHA");

    let excluded = |p: &str| {
        LINK_GUARD_EXCLUDED
            .iter()
            .any(|(pattern, _)| link_guard_excludes(p, pattern))
    };
    let top_level_in = |p: &str, dir: &str| {
        p.strip_prefix(dir)
            .is_some_and(|rest| !rest.is_empty() && !rest.contains('/'))
    };
    let scope: Vec<&String> = tracked
        .iter()
        .filter(|p| {
            (p.ends_with(".md") && !excluded(p))
                || (top_level_in(p, "config/") && p.ends_with(".toml"))
                || top_level_in(p, "docker/")
        })
        .collect();

    // Vacuity guard, part 1: named members of the scope, and named non-members.
    for must in [
        "README.md",
        "docs/README.md",
        "docs/HTTP-API.md",
        "docker/RAILWAY.md",
        "config/registry.example.toml",
        "docker/Dockerfile",
    ] {
        assert!(
            scope.iter().any(|p| p.as_str() == must),
            "sibling-link scope lost {must}: {scope:?}"
        );
    }
    for p in &scope {
        assert!(
            !p.starts_with("plans/") && !link_guard_excludes(p, "crates/*/CHANGELOG.md"),
            "{p} must stay out of the sibling-link scope"
        );
    }

    let mut violations = Vec::new();
    let mut readme_links = Vec::new();
    for source in &scope {
        // `docker/*` is all text today; a binary added there is not a document.
        let Ok(text) = std::fs::read_to_string(root.join(source.as_str())) else {
            continue;
        };
        if source.as_str() == "docs/README.md" {
            readme_links = sibling_links(&text, true);
        }
        violations.extend(sibling_link_violations(source, &text, &spec_ref));
    }

    // Violations first: after a spec bump this is the actionable list, and the
    // vacuity checks below would otherwise fire first with a less useful message.
    assert!(
        violations.is_empty(),
        "sibling-repository links that are not pinned:\n  {}\n\
         Spec links use the `ref:` in .spec-pin ({spec_ref}); after a spec bump, \
         re-point them as docs/MAINTAINING.md (\"Spec bumps\") shows. acdp-rs links use \
         a tag `acdp-v<X.Y.Z>` or a full SHA. See docs/README.md, \"Link convention\".",
        violations.join("\n  ")
    );

    // Vacuity guard, part 2: the docs index carries one inline link of each
    // pinned kind and the scanner SAW each -- an extractor that finds nothing
    // would otherwise pass every tree.
    let seen = |pred: &dyn Fn(&SiblingTarget) -> bool| {
        readme_links
            .iter()
            .any(|l| l.form == LinkForm::Inline && pred(&l.target))
    };
    assert!(
        seen(&|t| matches!(t, SiblingTarget::AtRef { repo, git_ref, .. }
            if repo == SPEC_REPO && *git_ref == spec_ref)),
        "the scanner found no inline spec link at the .spec-pin ref in docs/README.md: \
         {readme_links:?}"
    );
    assert!(
        seen(&|t| matches!(t, SiblingTarget::AtRef { repo, git_ref, .. }
            if repo == SDK_REPO && is_sdk_pin(git_ref))),
        "the scanner found no inline pinned acdp-rs link in docs/README.md: {readme_links:?}"
    );
    assert!(
        seen(
            &|t| matches!(t, SiblingTarget::DocsRs { version: Some(v), .. }
            if is_crate_version(v))
        ),
        "the scanner found no versioned docs.rs link in docs/README.md: {readme_links:?}"
    );
}

/// The pin guard's matcher on synthetic text: each rule fails alone, and what
/// is not a link is ignored.
#[test]
fn the_sibling_link_guard_rejects_what_it_must_and_ignores_what_it_should() {
    let pin = "9deb7e7bdabfa7416fcc0e25a7fcac6eb642b6dd";
    let gh = "https://github.com/agentcontextdistributionprotocol";
    let spec = format!("{gh}/agentcontextdistributionprotocol");
    let sdk = format!("{gh}/acdp-rs");
    let raw = "https://raw.githubusercontent.com/agentcontextdistributionprotocol";
    let check = |source: &str, text: &str| sibling_link_violations(source, text, pin);

    let good = [
        format!("[rfc]({spec}/blob/{pin}/rfcs/RFC-ACDP-0007-errors.md#4-envelope)"),
        format!("[dir]({spec}/tree/{pin}/registries) and [root]({spec}/tree/{pin})"),
        format!("[docs]({spec}/blob/{SPEC_DOCS_REF}/docs/overview.md)"),
        format!("[tag]({sdk}/blob/acdp-v0.14.3/docs/registry.md)"),
        format!("[sha](<{sdk}/tree/8a888edaa15c4475bbaeccff45567921e3153730/docs>)"),
        format!("[own]({gh}/acdp-registry-rs/blob/main/README.md)"),
        format!(
            "[own raw]({raw}/acdp-registry-rs/main/x.json) see {gh}/acdp-registry-rs/tree/main"
        ),
        format!("[issue]({sdk}/issues/1) and [repo]({spec})"),
        format!("[ci]({gh}/acdp-ci/blob/v1/README.md)"),
        "[docs.rs/acdp 0.14.3](https://docs.rs/acdp/0.14.3/acdp/) \
         [sub](https://docs.rs/acdp-client/0.14.3-rc.1/acdp_client/)"
            .to_string(),
        "[other](https://docs.rs/serde/latest/serde/) \
         [gist](https://gist.github.com/agentcontextdistributionprotocol/x/blob/main/y)"
            .to_string(),
        format!("in code: `{sdk}/blob/main/docs/x.md` and ``[x]({spec}/blob/main/rfcs)``"),
        format!("```\n[fenced]({sdk}/blob/main/docs/x.md)\n```"),
        format!("~~~text\n{spec}/blob/main/rfcs\n~~~"),
        // Positive twins of the www / HEAD / case / underscore / crate-form rules.
        "[w](https://www.github.com/agentcontextdistributionprotocol/acdp-rs/blob/acdp-v0.14.3/x.md)"
            .to_string(),
        format!("[own head]({gh}/acdp-registry-rs/BLOB/HEAD/README.md)"),
        "[u](https://docs.rs/acdp_client/0.14.3/acdp_client/) \
         [c](https://docs.rs/crate/acdp/0.14.3)"
            .to_string(),
        // Scheme-less `www.` as link TEXT is not a URL; glued into a longer
        // host it is not this host.
        "[www.github.com/agentcontextdistributionprotocol/acdp-rs](https://docs.rs/acdp/0.14.3/acdp/) \
         see notwww.github.com/agentcontextdistributionprotocol/acdp-rs/blob/main/x"
            .to_string(),
    ];
    for g in &good {
        assert_eq!(
            check("docs/a.md", g),
            Vec::<String>::new(),
            "must pass: {g}"
        );
    }
    assert_eq!(
        sibling_links(&good.join("\n"), true).len(),
        19,
        "the scanner must see the 19 sibling/acdp docs.rs URLs above (not serde, not \
         the gist host, not link text or a glued host, nothing in code): {:?}",
        sibling_links(&good.join("\n"), true)
    );

    let bad = [
        ("blob main", format!("[x]({sdk}/blob/main/docs/x.md)")),
        ("tree main", format!("[x]({sdk}/tree/main/docs)")),
        ("raw main", format!("[x]({raw}/acdp-rs/main/docs/x.md)")),
        ("github raw master", format!("[x]({sdk}/raw/master/x.json)")),
        (
            "refs/heads/main",
            format!("[x]({raw}/acdp-ci/refs/heads/main/x.sh)"),
        ),
        (
            "other sibling main",
            format!("[x]({gh}/acdp-ci/blob/main/README.md)"),
        ),
        (
            "spec main",
            format!("[x]({spec}/blob/main/rfcs/RFC-ACDP-0001.md)"),
        ),
        (
            "wrong spec sha",
            format!("[x]({spec}/blob/0000000000000000000000000000000000000000/rfcs/a.md)"),
        ),
        (
            "short spec sha",
            format!("[x]({spec}/blob/9deb7e7/rfcs/a.md)"),
        ),
        ("spec tag", format!("[x]({spec}/blob/v0.5.0/rfcs/a.md)")),
        (
            "SPEC_DOCS_REF outside docs/",
            format!("[x]({spec}/blob/{SPEC_DOCS_REF}/rfcs/a.md)"),
        ),
        (
            "SPEC_DOCS_REF on the repo root",
            format!("[x]({spec}/tree/{SPEC_DOCS_REF})"),
        ),
        ("sdk main", format!("[x]({sdk}/blob/main/docs/registry.md)")),
        (
            "sdk short sha",
            format!("[x]({sdk}/blob/8a888ed/docs/x.md)"),
        ),
        (
            "sdk bare v-tag",
            format!("[x]({sdk}/blob/v0.14.3/docs/x.md)"),
        ),
        (
            "reference-style definition",
            format!("[r]: {sdk}/blob/acdp-v0.14.3/docs/x.md"),
        ),
        (
            "bare url",
            format!("see {sdk}/blob/acdp-v0.14.3/docs/x.md for more"),
        ),
        ("autolink", format!("<{sdk}/blob/acdp-v0.14.3/docs/x.md>")),
        (
            "unversioned docs.rs",
            "[x](https://docs.rs/acdp/)".to_string(),
        ),
        (
            "docs.rs latest",
            "[x](https://docs.rs/acdp-client/latest/acdp_client/)".to_string(),
        ),
        (
            "docs.rs crate root",
            "[x](https://docs.rs/acdp)".to_string(),
        ),
        (
            "http scheme, blob main",
            "[x](http://github.com/agentcontextdistributionprotocol/acdp-rs/blob/main/x.md)"
                .to_string(),
        ),
        (
            "www host, blob main",
            "[x](https://www.github.com/agentcontextdistributionprotocol/acdp-rs/blob/main/x.md)"
                .to_string(),
        ),
        (
            "scheme-less www autolink",
            "see www.github.com/agentcontextdistributionprotocol/acdp-rs/blob/acdp-v0.14.3/x.md"
                .to_string(),
        ),
        (
            "other sibling HEAD",
            format!("[x]({gh}/acdp-ci/blob/HEAD/README.md)"),
        ),
        ("sdk HEAD", format!("[x]({sdk}/tree/HEAD/docs)")),
        ("upper-case BLOB main", format!("[x]({sdk}/BLOB/main/x.md)")),
        (
            "mixed-case Tree main",
            format!("[x]({gh}/acdp-ci/Tree/main)"),
        ),
        (
            "underscore crate name, latest",
            "[x](https://docs.rs/acdp_client/latest/acdp_client/)".to_string(),
        ),
        (
            "docs.rs/crate form, latest",
            "[x](https://docs.rs/crate/acdp/latest)".to_string(),
        ),
        (
            "docs.rs/crate form, no version",
            "[x](https://docs.rs/crate/acdp-client)".to_string(),
        ),
    ];
    for (label, b) in &bad {
        let found = check("docs/a.md", b);
        assert_eq!(
            found.len(),
            1,
            "{label}: `{b}` must produce exactly one violation, got {found:?}"
        );
    }

    // Non-markdown files: refs are checked, the inline-only rule is not, and
    // there are no code spans -- a backticked URL in a comment still counts.
    assert_eq!(
        check(
            "docker/Dockerfile",
            &format!("# see {sdk}/blob/acdp-v0.14.3/x")
        ),
        Vec::<String>::new()
    );
    assert_eq!(
        check("config/a.toml", &format!("# see {sdk}/blob/main/x")).len(),
        1
    );
    assert_eq!(
        check("docker/x.sh", &format!("# `{spec}/blob/main/rfcs`")).len(),
        1
    );
    assert_eq!(
        check(
            "config/a.toml",
            "# www.github.com/agentcontextdistributionprotocol/acdp-rs/blob/main/x"
        )
        .len(),
        1,
        "a scheme-less www URL in a comment is still read by someone"
    );

    // .spec-pin parsing on the real file's shape: the 64-hex digest is not the ref.
    let (r, repo) = spec_pin_ref_and_repo(&format!(
        "# comment\nrepository: {ACDP_ORG}/{SPEC_REPO}\nref: {pin}\n\
         conformance-digest: rfc6962-sha256:{}\n",
        "a".repeat(64)
    ));
    assert_eq!(r, pin);
    assert_eq!(repo, format!("{ACDP_ORG}/{SPEC_REPO}"));
    assert!(is_sdk_pin("acdp-v0.14.3") && !is_sdk_pin("acdp-v0.14") && !is_sdk_pin("main"));
    assert!(
        is_crate_version("0.14.3") && !is_crate_version("latest") && !is_crate_version("^0.14")
    );
}
