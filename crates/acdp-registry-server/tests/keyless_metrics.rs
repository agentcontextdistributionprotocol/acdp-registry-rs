//! RFC-ACDP-0021: the keyless issuer-key metrics on the wire.
//!
//! `acdp_registry_keyless_key_fetch_total{outcome}` and
//! `acdp_registry_keyless_key_stale_served_total` (docs/HTTP-API.md), read
//! from `GET /metrics` of an in-process keyless registry over the kit's
//! loopback issuer.
//!
//! Its own test binary, with one test: the `metrics` recorder is
//! process-global, so exact counts need a process no other test shares
//! (as `tests/metrics_integration.rs`).

#![cfg(feature = "storage-sqlite")]

mod keyless_kit;

use std::sync::atomic::Ordering;

use acdp_registry_core::metrics::{KEYLESS_KEY_FETCH_TOTAL, KEYLESS_KEY_STALE_SERVED_TOTAL};
use acdp_registry_sqlite::SqliteStore;
use acdp_registry_store::ExtendedRegistryStore;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use keyless_kit::{ed_key, Kit, Opts, SUBJECT};
use tower::ServiceExt;

async fn scrape(kit: &Kit) -> String {
    let resp = kit
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

/// The value of the series `name` whose labels are exactly `labels` (`""`
/// for none), or 0 when it has not been emitted.
fn series(text: &str, name: &str, labels: &str) -> f64 {
    let key = if labels.is_empty() {
        name.to_string()
    } else {
        format!("{name}{{{labels}}}")
    };
    text.lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| l.rsplit_once(' '))
        .find(|(k, _)| *k == key)
        .map_or(0.0, |(_, v)| v.parse().unwrap())
}

fn fetches(text: &str, outcome: &str) -> f64 {
    series(
        text,
        KEYLESS_KEY_FETCH_TOTAL,
        &format!("outcome=\"{outcome}\""),
    )
}

fn stale(text: &str) -> f64 {
    series(text, KEYLESS_KEY_STALE_SERVED_TOTAL, "")
}

async fn mint(kit: &Kit, controller: &str) {
    let req = Kit::request(&ed_key(), controller, kit.oidc(SUBJECT), Some(120));
    let (s, v) = kit.post(&req).await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_jwks_fetch_and_a_stale_serve_are_counted_on_metrics() {
    assert_eq!(
        KEYLESS_KEY_FETCH_TOTAL,
        "acdp_registry_keyless_key_fetch_total"
    );
    assert_eq!(
        KEYLESS_KEY_STALE_SERVED_TOTAL,
        "acdp_registry_keyless_key_stale_served_total"
    );
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::connect(&dir.path().join("r.sqlite"), 1)
        .await
        .unwrap();
    store.migrate().await.unwrap();
    let kit = Kit::new(
        store,
        Opts {
            metrics: true,
            ..Opts::default()
        },
    )
    .await;
    let controller = kit.controller("metrics", &[(kit.oidc_identity(SUBJECT), false)]);

    // Nothing fetched yet.
    let t = scrape(&kit).await;
    assert_eq!((fetches(&t, "ok"), stale(&t)), (0.0, 0.0), "{t}");

    // A cold cache: one JWKS fetch, `ok`.
    mint(&kit, &controller).await;
    assert_eq!(kit.outside.hits("jwks"), 1);
    let t = scrape(&kit).await;
    assert_eq!(fetches(&t, "ok"), 1.0, "{t}");
    assert_eq!(fetches(&t, "bad_status"), 0.0, "{t}");
    assert_eq!(stale(&t), 0.0, "{t}");

    // A fresh copy: no fetch, nothing counted.
    mint(&kit, &controller).await;
    assert_eq!(kit.outside.hits("jwks"), 1);
    let t = scrape(&kit).await;
    assert_eq!((fetches(&t, "ok"), stale(&t)), (1.0, 0.0), "{t}");

    // Past the 300 s TTL with the issuer answering 503: the refetch is a
    // `bad_status` failure and the stale copy (inside the 3600 s window)
    // answers, so the mint still succeeds.
    kit.outside.set_keys_status(503);
    kit.clock.fetch_add(301 * 1000, Ordering::SeqCst);
    mint(&kit, &controller).await;
    assert_eq!(kit.outside.hits("jwks"), 2);
    let t = scrape(&kit).await;
    assert_eq!(fetches(&t, "bad_status"), 1.0, "{t}");
    assert_eq!(fetches(&t, "ok"), 1.0, "{t}");
    assert_eq!(stale(&t), 1.0, "{t}");
    assert!(
        t.contains("# TYPE acdp_registry_keyless_key_fetch_total counter")
            && t.contains("# TYPE acdp_registry_keyless_key_stale_served_total counter"),
        "{t}"
    );
}
