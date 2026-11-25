use std::io;

use subtle::ConstantTimeEq;
use tokio::io::{AsyncRead, AsyncReadExt};

use super::super::cas::invalid;

pub(super) struct Request {
    pub method: String,
    pub path: String,
    pub length: u64,
}

pub(super) async fn read(
    stream: &mut (impl AsyncRead + Unpin),
    token: &str,
) -> io::Result<Request> {
    let mut bytes = Vec::with_capacity(1024);
    loop {
        bytes.push(stream.read_u8().await?);
        if bytes.ends_with(b"\r\n\r\n") {
            break;
        }
        if bytes.len() >= 16 * 1024 {
            return Err(invalid("HTTP header limit exceeded"));
        }
    }
    let mut headers = [httparse::EMPTY_HEADER; 32];
    let mut request = httparse::Request::new(&mut headers);
    if !request
        .parse(&bytes)
        .map_err(|_| invalid("malformed HTTP request"))?
        .is_complete()
    {
        return Err(invalid("incomplete HTTP request"));
    }
    if request.version != Some(1) {
        return Err(invalid("worker requires HTTP/1.1"));
    }
    let mut auth = None;
    let mut length = None;
    for header in request.headers {
        if header.name.eq_ignore_ascii_case("authorization") && auth.replace(header.value).is_some()
        {
            return Err(invalid("duplicate authorization"));
        }
        if header.name.eq_ignore_ascii_case("content-length") {
            if length.is_some() {
                return Err(invalid("duplicate content length"));
            }
            let value =
                std::str::from_utf8(header.value).map_err(|_| invalid("invalid content length"))?;
            if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(invalid("invalid content length"));
            }
            length = Some(
                value
                    .parse::<u64>()
                    .map_err(|_| invalid("invalid content length"))?,
            );
        }
        if header.name.eq_ignore_ascii_case("transfer-encoding")
            || header.name.eq_ignore_ascii_case("expect")
        {
            return Err(invalid(
                "chunked transfer and expectations are not supported",
            ));
        }
    }
    let expected = format!("Bearer {token}");
    if !bool::from(auth.unwrap_or_default().ct_eq(expected.as_bytes())) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "unauthorized",
        ));
    }
    let method = request
        .method
        .ok_or_else(|| invalid("missing HTTP method"))?
        .to_owned();
    if matches!(method.as_str(), "POST" | "PUT") && length.is_none() {
        return Err(invalid("content length required"));
    }
    Ok(Request {
        method,
        path: request
            .path
            .ok_or_else(|| invalid("missing request path"))?
            .to_owned(),
        length: length.unwrap_or(0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn every_request_requires_auth_and_unambiguous_framing() {
        for path in ["/v1/health", "/v1/actions", "/v1/cas/abcd"] {
            let bytes = format!("GET {path} HTTP/1.1\r\n\r\n");
            assert_eq!(
                read(&mut bytes.as_bytes(), "secret")
                    .await
                    .err()
                    .unwrap()
                    .kind(),
                io::ErrorKind::PermissionDenied
            );
        }
        let bytes = b"GET /v1/health HTTP/1.1\r\nAuthorization: Bearer secret\r\n\r\n";
        assert_eq!(
            read(&mut bytes.as_slice(), "secret").await.unwrap().path,
            "/v1/health"
        );
        for extra in [
            "Content-Length: 0\r\nContent-Length: 1",
            "Transfer-Encoding: chunked",
            "Authorization: Bearer other",
        ] {
            let bytes = format!(
                "GET /v1/health HTTP/1.1\r\nAuthorization: Bearer secret\r\n{extra}\r\n\r\n"
            );
            assert!(read(&mut bytes.as_bytes(), "secret").await.is_err());
        }
    }
}
