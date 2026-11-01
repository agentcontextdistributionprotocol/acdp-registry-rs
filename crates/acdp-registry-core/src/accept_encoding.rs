//! `Accept-Encoding` normalisation in front of `GET /changes` compression.
//!
//! `/changes` is the only compressed route, and gzip is the only coding
//! compiled in (tower-http `compression-gzip`). Since 0.7.0, tower-http's
//! `CompressionLayer` answers **406 Not Acceptable**, overwriting whatever
//! status the handler returned, whenever an `Accept-Encoding` header rules
//! out identity and offers nothing it can produce (`identity;q=0`, `*;q=0`,
//! `br, identity;q=0`, `gzip;q=0, identity;q=0`). It has no switch for this.
//!
//! The registry never answers 406 on `/changes`: a feed consumer with an
//! over-strict header keeps the feed, and an ACDP 400 is never masked. RFC
//! 9110 §12.5.3 lets an origin server send a response without any content
//! coding when nothing listed is acceptable. [`normalize_accept_encoding`]
//! runs before the compression layer and reduces the header to the only two
//! outcomes we can serve:
//!
//! - gzip acceptable (an explicit `gzip` / `x-gzip` with q > 0, or `*` with
//!   q > 0 while gzip is not listed explicitly): the header becomes `gzip`;
//! - anything else, including a missing, empty or malformed header: the
//!   header is removed, so the compression layer picks identity and cannot
//!   reach its 406 branch.
//!
//! Codings are compared case-insensitively. A member whose coding is empty
//! or whose `q` does not parse as an RFC 9110 §12.4.2 qvalue is ignored, as
//! is a header value that is not visible ASCII.

use axum::extract::Request;
use axum::http::header::ACCEPT_ENCODING;
use axum::http::{HeaderMap, HeaderValue};

/// Whether the `Accept-Encoding` headers in `headers` accept gzip.
///
/// All header lines are read as one list. Absent, empty or entirely
/// malformed headers accept nothing (the caller then serves identity).
#[must_use]
pub fn accepts_gzip(headers: &HeaderMap) -> bool {
    // Highest q seen for an explicit gzip / x-gzip, and for `*`.
    let mut gzip: Option<u16> = None;
    let mut star: Option<u16> = None;
    for value in headers.get_all(ACCEPT_ENCODING) {
        let Ok(value) = value.to_str() else { continue };
        for member in value.split(',') {
            let Some((coding, q)) = parse_member(member) else {
                continue;
            };
            let slot =
                if coding.eq_ignore_ascii_case("gzip") || coding.eq_ignore_ascii_case("x-gzip") {
                    &mut gzip
                } else if coding == "*" {
                    &mut star
                } else {
                    continue;
                };
            *slot = Some(slot.map_or(q, |seen| seen.max(q)));
        }
    }
    match gzip {
        Some(q) => q > 0,
        None => star.is_some_and(|q| q > 0),
    }
}

/// One list member: `coding *( OWS ";" OWS name "=" value )`. Returns the
/// coding and its weight in thousandths (1000 when no `q` is given), or
/// `None` when the member is empty or its `q` is malformed.
fn parse_member(member: &str) -> Option<(&str, u16)> {
    let mut parts = member.split(';');
    let coding = parts.next()?.trim();
    if coding.is_empty() {
        return None;
    }
    let mut q = 1000;
    for param in parts {
        let (name, value) = param.split_once('=')?;
        if name.trim().eq_ignore_ascii_case("q") {
            q = parse_qvalue(value.trim())?;
        }
    }
    Some((coding, q))
}

/// RFC 9110 §12.4.2: `qvalue = ( "0" [ "." 0*3DIGIT ] ) / ( "1" [ "." 0*3("0") ] )`,
/// in thousandths.
fn parse_qvalue(s: &str) -> Option<u16> {
    let (int, frac) = match s.split_once('.') {
        Some((int, frac)) => (int, frac),
        None => (s, ""),
    };
    if frac.len() > 3 || !frac.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut thousandths: u16 = 0;
    for (i, b) in frac.bytes().enumerate() {
        thousandths += u16::from(b - b'0') * [100, 10, 1][i];
    }
    match int {
        "0" => Some(thousandths),
        "1" if thousandths == 0 => Some(1000),
        _ => None,
    }
}

/// `map_request` middleware: rewrite `Accept-Encoding` to `gzip` when gzip
/// is acceptable, otherwise remove it. See the module docs.
pub async fn normalize_accept_encoding(mut req: Request) -> Request {
    let gzip = accepts_gzip(req.headers());
    let headers = req.headers_mut();
    headers.remove(ACCEPT_ENCODING);
    if gzip {
        headers.insert(ACCEPT_ENCODING, HeaderValue::from_static("gzip"));
    }
    req
}

#[cfg(test)]
mod tests {
    use super::*;

    fn accepts(values: &[&[u8]]) -> bool {
        let mut headers = HeaderMap::new();
        for v in values {
            headers.append(ACCEPT_ENCODING, HeaderValue::from_bytes(v).unwrap());
        }
        accepts_gzip(&headers)
    }

    #[test]
    fn gzip_accepted() {
        for v in [
            "gzip",
            "GZIP",
            "x-gzip",
            "X-Gzip",
            "gzip;q=0.5",
            "gzip; q=1",
            "gzip ;Q=1.000",
            "*",
            "*;q=0.1",
            "*;q=0.001",
            "br, gzip;q=0.2",
            "gzip, deflate, br, zstd",
            "identity;q=0, gzip",
            "gzip;q=0, x-gzip;q=0.3",
            "gzip;level=9",
            "gzip;q=abc, *",
            ", gzip",
        ] {
            assert!(accepts(&[v.as_bytes()]), "{v:?} should accept gzip");
        }
    }

    #[test]
    fn gzip_not_accepted() {
        for v in [
            "",
            "   ",
            ",",
            "identity",
            "identity;q=0",
            "*;q=0",
            "br",
            "br, identity;q=0",
            "gzip;q=0",
            "gzip;q=0.000",
            "gzip;q=0, identity;q=0",
            "gzip;q=0, *",
            "*, gzip;q=0",
            "gzip;q=abc",
            "gzip;q=",
            "gzip;q=1.5",
            "gzip;q=2",
            "gzip;q=0.0001",
            "gzip;q=-1",
            "gzip;q",
            "gzipx",
            "gzip-ish",
            ";q=1",
        ] {
            assert!(!accepts(&[v.as_bytes()]), "{v:?} should not accept gzip");
        }
    }

    #[test]
    fn header_lines_are_one_list() {
        assert!(accepts(&[b"br", b"gzip"]));
        assert!(!accepts(&[b"gzip;q=0", b"*"]));
        // A line that is not visible ASCII is ignored; the other still counts.
        assert!(accepts(&[b"gz\xffip", b"gzip"]));
        assert!(!accepts(&[b"gz\xffip"]));
        assert!(!accepts(&[]));
    }

    #[test]
    fn qvalues() {
        assert_eq!(parse_qvalue("0"), Some(0));
        assert_eq!(parse_qvalue("0."), Some(0));
        assert_eq!(parse_qvalue("0.5"), Some(500));
        assert_eq!(parse_qvalue("0.05"), Some(50));
        assert_eq!(parse_qvalue("0.123"), Some(123));
        assert_eq!(parse_qvalue("1"), Some(1000));
        assert_eq!(parse_qvalue("1.000"), Some(1000));
        for bad in [
            "", ".5", "1.001", "0.1234", "01", "2", "0,5", "0.a", "+1", "1e0",
        ] {
            assert_eq!(parse_qvalue(bad), None, "{bad:?}");
        }
    }

    #[tokio::test]
    async fn normalizer_rewrites_or_removes() {
        let run = |values: &'static [&'static str]| async move {
            let mut req = Request::new(axum::body::Body::empty());
            for v in values {
                req.headers_mut()
                    .append(ACCEPT_ENCODING, HeaderValue::from_static(v));
            }
            let req = normalize_accept_encoding(req).await;
            req.headers()
                .get_all(ACCEPT_ENCODING)
                .iter()
                .map(|v| v.to_str().unwrap().to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(run(&["br;q=1, GZIP;q=0.4"]).await, ["gzip"]);
        assert_eq!(run(&["br", "*"]).await, ["gzip"]);
        assert!(run(&["identity;q=0"]).await.is_empty());
        assert!(run(&["br"]).await.is_empty());
        assert!(run(&[]).await.is_empty());
    }
}
