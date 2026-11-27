use std::io;

use subtle::ConstantTimeEq;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

use super::IDLE;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Method {
    Get,
    Head,
    Put,
}

pub(super) struct Request {
    pub method: Method,
    pub key: String,
    pub length: u64,
    pub expect: bool,
    pub keep_alive: bool,
    pub zstd: bool,
}

pub(super) fn valid_key(key: &str) -> bool {
    let Some(digest) = key
        .strip_prefix("v1.cas.")
        .or_else(|| key.strip_prefix("v1.ac."))
    else {
        return false;
    };
    digest.len() == 64
        && digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(super) async fn read(
    socket: &mut TcpStream,
    buffer: &mut [u8],
    auth: &[u8],
    capacity: u64,
    mut end: usize,
) -> Result<Option<(Request, usize, usize)>, u16> {
    timeout(IDLE, async {
        loop {
            let mut headers = [httparse::EMPTY_HEADER; 100];
            let mut parsed = httparse::Request::new(&mut headers);
            match parsed.parse(&buffer[..end]) {
                Ok(httparse::Status::Complete(start)) => {
                    return validate(&parsed, &buffer[..start], auth, capacity)
                        .map(|request| Some((request, start, end)));
                }
                Ok(httparse::Status::Partial) if end < buffer.len() => {}
                _ => return Err(400),
            }
            let read = socket.read(&mut buffer[end..]).await.map_err(|_| 400u16)?;
            if read == 0 {
                return if end == 0 { Ok(None) } else { Err(400) };
            }
            end += read;
        }
    })
    .await
    .unwrap_or(Err(408))
}

fn validate(
    parsed: &httparse::Request<'_, '_>,
    raw: &[u8],
    auth: &[u8],
    capacity: u64,
) -> Result<Request, u16> {
    let values = |name: &'static str| {
        parsed
            .headers
            .iter()
            .filter(move |header| header.name.eq_ignore_ascii_case(name))
            .map(|header| untrimmed_value(header, raw))
    };
    let mut authorization = values("authorization");
    let supplied = authorization.next().unwrap_or_default();
    let authenticated = supplied.ct_eq(auth);
    if !bool::from(authenticated) || authorization.next().is_some() {
        return Err(401);
    }
    let key = parsed
        .path
        .and_then(|path| path.strip_prefix('/'))
        .filter(|key| valid_key(key))
        .ok_or(400u16)?;
    let method = match parsed.method {
        Some("GET") => Method::Get,
        Some("HEAD") => Method::Head,
        Some("PUT") => Method::Put,
        _ => return Err(405),
    };
    let mut lengths = values("content-length");
    let length = lengths.next();
    if lengths.next().is_some() || values("transfer-encoding").next().is_some() {
        return Err(400);
    }
    let length = match length {
        Some(value) => {
            if value.is_empty() || !value.iter().all(u8::is_ascii_digit) {
                return Err(400);
            }
            let mut number = 0u64;
            for digit in value {
                number = number
                    .checked_mul(10)
                    .and_then(|value| value.checked_add(u64::from(digit - b'0')))
                    .ok_or(413u16)?;
                if number > capacity {
                    return Err(413);
                }
            }
            number
        }
        None if method == Method::Put => return Err(411),
        None => 0,
    };
    if method != Method::Put && length != 0 {
        return Err(400);
    }
    Ok(Request {
        method,
        key: key.to_owned(),
        length,
        expect: values("expect").any(|value| value.eq_ignore_ascii_case(b"100-continue")),
        keep_alive: parsed.version == Some(1)
            && !values("connection").any(|value| {
                value
                    .split(|byte| *byte == b',')
                    .any(|token| token.trim_ascii().eq_ignore_ascii_case(b"close"))
            }),
        zstd: accepts_zstd(values("accept-encoding")),
    })
}

fn accepts_zstd<'a>(values: impl Iterator<Item = &'a [u8]>) -> bool {
    let mut explicit = None;
    let mut wildcard = false;
    for value in values.filter_map(|value| std::str::from_utf8(value).ok()) {
        for coding in value.split(',') {
            let mut parts = coding.split(';');
            let name = parts.next().unwrap_or_default().trim();
            let enabled = parts.all(|parameter| {
                parameter.split_once('=').is_some_and(|(name, value)| {
                    name.trim().eq_ignore_ascii_case("q")
                        && value
                            .trim()
                            .parse::<f32>()
                            .is_ok_and(|value| value > 0.0 && value <= 1.0)
                })
            });
            if name.eq_ignore_ascii_case("zstd") {
                explicit = Some(explicit.unwrap_or(true) && enabled);
            } else if name == "*" {
                wildcard |= enabled;
            }
        }
    }
    explicit.unwrap_or(wildcard)
}

fn untrimmed_value<'a>(header: &httparse::Header<'_>, raw: &'a [u8]) -> &'a [u8] {
    // httparse trims trailing whitespace, but credentials and decimal lengths
    // in this protocol must match the original header value exactly.
    let offset = header.value.as_ptr() as usize - raw.as_ptr() as usize;
    let tail = &raw[offset..];
    let length = tail
        .iter()
        .position(|byte| matches!(byte, b'\r' | b'\n'))
        .unwrap_or(tail.len());
    &tail[..length]
}

pub(super) async fn reply(
    socket: &mut TcpStream,
    status: u16,
    length: u64,
    keep_alive: bool,
) -> io::Result<()> {
    let reason = match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        411 => "Length Required",
        413 => "Content Too Large",
        503 => "Service Unavailable",
        507 => "Insufficient Storage",
        _ => "Internal Server Error",
    };
    let connection = if keep_alive { "keep-alive" } else { "close" };
    let header = format!("HTTP/1.1 {status} {reason}\r\nContent-Length: {length}\r\nVary: Accept-Encoding\r\nConnection: {connection}\r\n\r\n");
    timeout(IDLE, socket.write_all(header.as_bytes())).await?
}
