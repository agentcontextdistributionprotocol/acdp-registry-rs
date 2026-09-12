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
///    has been deleted from the router — for that, a wire test that actually calls the
/// endpoint is the only real guard. Two different failure modes; this one covers
/// the direction that has actually bitten twice.
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
