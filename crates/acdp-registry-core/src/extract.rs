//! Extractor rejections that speak RFC-ACDP-0007 §5 (A3).
//!
//! axum's built-in `Json` and `Query` rejections answer with a **plain-text**
//! body: `"Failed to parse the request body as JSON: key must be a string at
//! line 1 column 3"`, `"Failed to deserialize the JSON body into the target
//! type: agent_id: invalid type: integer `123`"`. The outermost media-type
//! backstop then stamps `application/acdp+json` onto that prose, so the wire
//! carries a content-type promising an ACDP error envelope over a body that is
//! not JSON at all, has no `error.code`, and leaks serde type paths and field
//! names from internal structs.
//!
//! These wrappers keep axum's extractors and replace only the rejection.
//!
//! # Why not the obvious mechanisms
//!
//! - `impl From<JsonRejection> for RegistryError` — the orphan rule forces that
//!   impl into `acdp-registry-types`, i.e. a different crate than the problem.
//! - Mapping onto `RegistryError` — it has no 415-bearing variant, so 415 and
//!   422 would both collapse to 400. That collapse would be an artifact of the
//!   chosen mechanism, not a decision: RFC 9110 §15.5.16 makes 415 correct for
//!   an unsupported media type, and this is a protocol that *mandates* one.
//! - `axum-extra`'s `WithRejection` — not a dependency, and adding one enters
//!   the `deny.toml` gate for a problem solvable in forty lines here.
//!
//! So: a local rejection type implementing `IntoResponse` directly, carrying
//! the ORIGINAL status.

use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{FromRequest, FromRequestParts, Request};
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::de::DeserializeOwned;

use acdp_registry_types::error::{WireError, WireErrorBody};

/// A rejection rendered as an RFC-ACDP-0007 §5 envelope **at its original
/// status**.
pub struct AcdpRejection {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl IntoResponse for AcdpRejection {
    fn into_response(self) -> Response {
        // Built from the public `WireError`/`WireErrorBody` rather than a
        // hand-rolled `json!`, so the envelope shape -- including `details`
        // being ABSENT rather than `null` -- cannot drift from what
        // `RegistryError::into_response` emits.
        let body = WireError {
            error: WireErrorBody {
                code: self.code.to_string(),
                message: self.message,
                details: None,
            },
        };
        (self.status, Json(body)).into_response()
    }
}

/// `axum::Json`, but rejecting with a §5 envelope.
pub struct AcdpJson<T>(pub T);

impl<T, S> FromRequest<S> for AcdpJson<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = AcdpRejection;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match Json::<T>::from_request(req, state).await {
            Ok(Json(v)) => Ok(AcdpJson(v)),
            Err(rej) => Err(AcdpRejection {
                // The rejection's OWN status, never a hard-coded 400. Both
                // rejection enums are `#[non_exhaustive]`, so the catch-all arm
                // below is mandatory -- and `JsonRejection::BytesRejection`
                // wraps `LengthLimitError`, which is a **413**. Hard-coding 400
                // would silently downgrade an oversized body on `/auth/*`, an
                // observable status change on exactly the path the 413 envelope
                // work exists to make conformant.
                status: rej.status(),
                code: json_code(&rej),
                message: json_message(&rej).to_string(),
            }),
        }
    }
}

/// `axum::extract::Query`, but rejecting with a §5 envelope.
pub struct AcdpQuery<T>(pub T);

impl<T, S> FromRequestParts<S> for AcdpQuery<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = AcdpRejection;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        match axum::extract::Query::<T>::from_request_parts(parts, state).await {
            Ok(axum::extract::Query(v)) => Ok(AcdpQuery(v)),
            Err(rej) => Err(AcdpRejection {
                status: rej.status(),
                code: query_code(&rej),
                message: "invalid query string".to_string(),
            }),
        }
    }
}

/// Map a `JsonRejection` to a §5 `code`.
///
/// Every code here is inside the canonical 25-code RFC-ACDP-0007 §5 registry
/// (`acdp_primitives::error::AcdpError::from_wire_error`). That is deliberate:
/// a code outside it maps to the client's forward-compat catch-all and loses
/// typing, so it is a cost to pay only where no canonical code is honest.
fn json_code(rej: &JsonRejection) -> &'static str {
    match rej {
        // A body that is not JSON, or is JSON that does not fit the target
        // shape. Both are `schema_violation` per the status/code table:
        // "Malformed body, missing field, schema mismatch."
        // The one code here OUTSIDE the canonical 25-code RFC-ACDP-0007 §5
        // registry, minted deliberately rather than by oversight.
        //
        // The canon has no code for a media-type failure, and
        // `WireErrorBody::code` is a required `String`, so there is no
        // "envelope without a code" to fall back on. The nearest canonical
        // option, `schema_violation`, is documented as "malformed body, missing
        // field, schema mismatch" -- and on a 415 the body was never parsed at
        // all, so it would state something false, make 415 indistinguishable
        // from 400 at the code level, and become unfixable once clients coded
        // against `AcdpError::SchemaViolation`.
        //
        // An unrecognised code costs strictly less: `from_wire_error` routes it
        // to `AcdpError::Registry(wire)`, explicitly "for forward
        // compatibility", keeping the 415 and the message and losing only the
        // typed variant. The name follows the canon's own `unsupported_*`
        // idiom. Divergence tracked upstream as acdp-rs#268; see DECISIONS.md
        // for the standing precedent this set.
        JsonRejection::MissingJsonContentType(_) => "unsupported_media_type",
        JsonRejection::JsonDataError(_) | JsonRejection::JsonSyntaxError(_) => "schema_violation",
        // `BytesRejection` is body-length territory and arrives as 413.
        JsonRejection::BytesRejection(_) => "payload_too_large",
        // `#[non_exhaustive]`: a future variant keeps its own status and gets
        // the generic body code rather than being forced to 400 or to 500.
        _ => "schema_violation",
    }
}

fn query_code(_rej: &QueryRejection) -> &'static str {
    // `QueryRejection` is `#[non_exhaustive]` with one variant today
    // (`FailedToDeserializeQueryString`, a 400). A malformed query string is a
    // malformed request.
    "schema_violation"
}

/// A stable, protocol-level message for a `JsonRejection`.
///
/// **Deliberately NOT `rej.body_text()`**, which is what the axum default
/// emits and what this phase exists to stop sending:
///
/// ```text
/// Failed to parse the request body as JSON: key must be a string at line 1 column 3
/// Failed to deserialize the JSON body into the target type: agent_id: invalid
///   type: integer `123`, expected a string at line 1 column 15
/// ```
///
/// Three problems with putting that on the wire. It names the serde operation
/// and the *target type* rather than the protocol, so it describes our
/// implementation instead of the client's mistake. It carries byte offsets into
/// a body the client already has and we do not otherwise echo. And it is
/// unstable: the text is axum's and serde's to change, so any client or
/// conformance fixture matching on it breaks on a dependency bump, which is
/// precisely the kind of accidental contract a wire protocol should not grow.
///
/// These messages are ours, stable, and describe the request rather than the
/// parser.
fn json_message(rej: &JsonRejection) -> &'static str {
    match rej {
        JsonRejection::JsonSyntaxError(_) => "request body is not valid JSON",
        JsonRejection::JsonDataError(_) => {
            "request body does not match the schema for this endpoint"
        }
        // The ruling specified this message VERBATIM, so it is pinned here as a
        // literal rather than taken from `rej.body_text()`. Same string axum
        // emits today, but ours now: if axum rewords its rejection, the wire
        // does not silently follow.
        //
        // NOTE, raised with the coordinator rather than acted on unilaterally:
        // this message names only `application/json`, while RFC-ACDP-0007
        // mandates `application/acdp+json`. Both are accepted (axum's `Json`
        // matches any `+json` structured suffix, pinned by
        // `the_acdp_media_type_is_accepted_not_rejected`), so the message is
        // INCOMPLETE rather than false -- a client following it literally sends
        // a media type that works but is not the one the RFC names. A one-line
        // change if the recommendation is taken.
        JsonRejection::MissingJsonContentType(_) => {
            "Expected request with `Content-Type: application/json`"
        }
        JsonRejection::BytesRejection(_) => "request body exceeds the configured limit",
        _ => "request body could not be read",
    }
}
