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
pub enum AcdpRejection {
    /// Rendered as a §5 envelope at `status`.
    Enveloped {
        status: StatusCode,
        code: &'static str,
        message: String,
    },
    /// Passed through to axum's own rendering, unchanged.
    ///
    /// **Used for exactly one case: the 415 from a missing or wrong
    /// `Content-Type`.** Not an oversight and not a design preference -- the
    /// §5 code for a 415 is an open question escalated to the project owner,
    /// because the canonical registry
    /// (`acdp_primitives::error::AcdpError::from_wire_error`) has 25 codes and
    /// none of them describes a media-type failure, so answering one means
    /// minting a code the canon lacks. That is a policy question about this
    /// repo's relationship to upstream, not a technical one.
    ///
    /// `WireErrorBody::code` is a required `String`, so there is no
    /// "envelope without a code" option to fall back on. Until the question is
    /// settled this case keeps the behaviour it has always had; the status is
    /// 415 either way, so nothing observable moves when the ruling lands except
    /// the body gaining an envelope.
    Passthrough(Response),
}

impl IntoResponse for AcdpRejection {
    fn into_response(self) -> Response {
        match self {
            // Built from the public `WireError`/`WireErrorBody` rather than a
            // hand-rolled `json!`, so the envelope shape -- including `details`
            // being ABSENT rather than `null` -- cannot drift from what
            // `RegistryError::into_response` emits.
            AcdpRejection::Enveloped {
                status,
                code,
                message,
            } => {
                let body = WireError {
                    error: WireErrorBody {
                        code: code.to_string(),
                        message,
                        details: None,
                    },
                };
                (status, Json(body)).into_response()
            }
            AcdpRejection::Passthrough(resp) => resp,
        }
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
            // HELD pending a ruling on the §5 code for 415 -- see
            // `AcdpRejection::Passthrough`. Everything else is enveloped now:
            // stamping `application/acdp+json` onto plain text with no
            // `error.code` is a live conformance defect and fixing 400/422/query
            // does not depend on the open question.
            Err(rej) if matches!(rej, JsonRejection::MissingJsonContentType(_)) => {
                Err(AcdpRejection::Passthrough(rej.into_response()))
            }
            Err(rej) => Err(AcdpRejection::Enveloped {
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
            Err(rej) => Err(AcdpRejection::Enveloped {
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
        JsonRejection::MissingJsonContentType(_) => {
            "expected Content-Type: application/acdp+json (application/json is also accepted)"
        }
        JsonRejection::BytesRejection(_) => "request body exceeds the configured limit",
        _ => "request body could not be read",
    }
}
