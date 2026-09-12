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
    assert!(
        spans.len() > 100,
        "only {} backticked spans found in {} — the scanner is broken, so every \
         check below would pass vacuously",
        spans.len(),
        doc_path.display()
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
    assert!(
        cited_paths.len() >= 8,
        "only {} crate source paths cited — expected the document to reference at \
         least 8; the scanner or the document changed shape: {cited_paths:?}",
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
    assert!(
        idents.len() >= 20,
        "only {} backticked identifiers extracted — expected at least 20: \
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
    assert!(
        checked >= 8,
        "found only {checked} per-crate changelogs — expected at least 8; the \
         walk is broken and the staleness check below proves nothing"
    );
    stale.sort();
    assert!(
        stale.is_empty(),
        "the workspace is at {version} but these crates' changelogs have no \
         `{heading}` section: {stale:?}. Release notes are delegated to these \
         files, so a gap here means the release is undocumented everywhere."
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

    assert!(
        pages.len() >= 8,
        "found only {} documents under docs/ — the walk is broken and the check \
         below would pass vacuously: {pages:?}",
        pages.len()
    );

    let unlisted: Vec<&String> = pages.iter().filter(|p| !index.contains(*p)).collect();
    assert!(
        unlisted.is_empty(),
        "these documents exist under docs/ but are absent from the Map table in \
         docs/README.md, so nothing links to them: {unlisted:?}"
    );
}
