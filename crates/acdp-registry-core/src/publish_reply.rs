//! The success reply of `POST /contexts` (RFC-ACDP-0003 §4 and §6.2).
//!
//! A fresh publish is `201 Created`; an idempotent replay (same
//! `(agent_id, Idempotency-Key)` and same `content_hash`) is `200 OK` with the
//! original response. Both carry `Location`: the canonical retrieval URL, a
//! path-relative `/contexts/` followed by the `ctx_id` percent-encoded as ONE
//! path segment. The handler learns which case it is from the SDK's
//! `PublishCommitOutcome`, never by looking the key up itself (that would race).

use acdp::types::publish::PublishResponse;
use acdp_registry_types::RegistryError;
use axum::http::header::LOCATION;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::Json;

/// What `publish` hands back to axum: status, headers (`Location`), body.
pub type PublishReply = (StatusCode, HeaderMap, Json<PublishResponse>);

/// The `Location` value for a context: `/contexts/` plus `ctx_id` percent-encoded
/// with the unreserved set only (`A-Za-z0-9-_.~` kept, everything else as
/// uppercase `%XX`). `acdp://host/uuid` becomes `acdp%3A%2F%2Fhost%2Fuuid`, the
/// form conformance fixture `pub-007` requires.
pub fn location_for(ctx_id: &str) -> String {
    let mut out = String::with_capacity("/contexts/".len() + ctx_id.len() * 3);
    out.push_str("/contexts/");
    for b in ctx_id.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(char::from(b));
            }
            _ => {
                out.push('%');
                out.push(char::from(b"0123456789ABCDEF"[usize::from(b >> 4)]));
                out.push(char::from(b"0123456789ABCDEF"[usize::from(b & 0x0F)]));
            }
        }
    }
    out
}

/// The status for a successful publish: `201 Created` for a fresh one (RFC-ACDP-0003
/// §4), `200 OK` for an idempotent replay (§6.2; idem-002 forbids `201` there).
pub fn success_status(is_replay: bool) -> StatusCode {
    if is_replay {
        StatusCode::OK
    } else {
        StatusCode::CREATED
    }
}

/// Build the reply for an accepted publish.
pub fn publish_reply(
    is_replay: bool,
    response: PublishResponse,
) -> Result<PublishReply, RegistryError> {
    let location = HeaderValue::from_str(&location_for(response.ctx_id.as_str()))
        .map_err(|e| RegistryError::Internal(format!("publish Location header: {e}")))?;
    let mut headers = HeaderMap::new();
    headers.insert(LOCATION, location);
    Ok((success_status(is_replay), headers, Json(response)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(s: &str) -> String {
        let b = s.as_bytes();
        let mut out = Vec::new();
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'%' {
                out.push(u8::from_str_radix(&s[i + 1..i + 3], 16).unwrap());
                i += 3;
            } else {
                out.push(b[i]);
                i += 1;
            }
        }
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn a_fresh_publish_is_created_and_a_replay_is_ok() {
        assert_eq!(success_status(false), StatusCode::CREATED);
        assert_eq!(success_status(true), StatusCode::OK);
    }

    #[test]
    fn a_ctx_id_is_encoded_as_one_path_segment() {
        let id = "acdp://registry.example.com/550e8400-e29b-41d4-a716-446655440000";
        assert_eq!(
            location_for(id),
            "/contexts/acdp%3A%2F%2Fregistry.example.com%2F550e8400-e29b-41d4-a716-446655440000"
        );
    }

    #[test]
    fn the_encoding_round_trips_and_uses_uppercase_hex() {
        for id in ["acdp://a.b/c", "acdp://host:8080/x y", "~-_.AZaz09", "é/💥"] {
            let loc = location_for(id);
            let tail = loc.strip_prefix("/contexts/").expect("prefix");
            assert_eq!(decode(tail), id);
            assert!(!tail.contains('/'), "a slash would split the path segment");
            let b = tail.as_bytes();
            for i in b
                .iter()
                .enumerate()
                .filter(|(_, c)| **c == b'%')
                .map(|(i, _)| i)
            {
                assert!(
                    !b[i + 1..i + 3].iter().any(u8::is_ascii_lowercase),
                    "hex digits must be uppercase: {tail}"
                );
            }
        }
        assert_eq!(location_for("a:b"), "/contexts/a%3Ab");
        assert_eq!(location_for("a b"), "/contexts/a%20b");
        assert_eq!(location_for("\u{7f}"), "/contexts/%7F");
        assert_eq!(location_for("~"), "/contexts/~");
    }
}
