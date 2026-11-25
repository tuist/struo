mod request;
mod service;

use std::io;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio_util::sync::CancellationToken;

use super::cas::{invalid, parse_digest, Cas};
use super::execution::Executor;
use super::{Request, PROTOCOL_VERSION};

pub(super) use service::serve;

async fn connection(
    stream: &mut BufReader<TcpStream>,
    root: &std::path::Path,
    token: &str,
    metadata_limit: usize,
    cas: &mut Cas,
    executor: &mut Executor,
) -> io::Result<()> {
    let request = tokio::time::timeout(Duration::from_secs(30), request::read(stream, token))
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "worker header timeout"))??;
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/v1/health") if request.length == 0 => {
            let body = serde_json::to_vec(
                &serde_json::json!({"version":PROTOCOL_VERSION,"environment":executor.options.environment}),
            )?;
            respond(stream.get_mut(), 200, &body).await
        }
        ("POST", "/v1/cas/missing") => {
            let body = read_metadata(stream, request.length, metadata_limit).await?;
            let digests: Vec<struo_cas::Digest> = serde_json::from_slice(&body)?;
            drop(body);
            let missing: Vec<_> = digests
                .into_iter()
                .filter(|digest| !cas.path(digest).is_file())
                .collect();
            let response = serde_json::to_vec(&missing)?;
            if response.len() > metadata_limit {
                return Err(invalid("CAS response metadata limit exceeded"));
            }
            respond(stream.get_mut(), 200, &response).await
        }
        ("POST", "/v1/cas/pull") => {
            pull(stream, root, request.length, metadata_limit, cas, executor).await
        }
        (method @ ("HEAD" | "GET" | "PUT"), path) if path.starts_with("/v1/cas/") => {
            let digest = parse_digest(&path[8..])?;
            if method == "PUT" {
                if !cas.path(&digest).is_file() {
                    cas.make_room(request.length)?;
                }
                if request.length > cas.capacity {
                    return Err(invalid("CAS capacity exceeded"));
                }
                let temporary = tempfile::NamedTempFile::new_in(root)?;
                let mut file = tokio::fs::File::from_std(temporary.reopen()?);
                let received = tokio::time::timeout(
                    Duration::from_secs(300),
                    tokio::io::copy(&mut (&mut *stream).take(request.length), &mut file),
                )
                .await
                .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "worker upload timeout"))??;
                file.flush().await?;
                if received != request.length {
                    return Err(invalid("truncated blob upload"));
                }
                cas.publish(temporary.path(), Some(digest))?;
                respond(stream.get_mut(), 200, b"").await
            } else {
                if request.length != 0 {
                    return Err(invalid("unexpected request body"));
                }
                let mut file = tokio::fs::File::open(cas.path(&digest)).await?;
                let length = file.metadata().await?.len();
                header(stream.get_mut(), 200, length).await?;
                if method == "GET" {
                    tokio::time::timeout(
                        Duration::from_secs(300),
                        tokio::io::copy(&mut file, stream.get_mut()),
                    )
                    .await
                    .map_err(|_| {
                        io::Error::new(io::ErrorKind::TimedOut, "worker download timeout")
                    })??;
                }
                Ok(())
            }
        }
        ("POST", "/v1/actions") => {
            let body = read_metadata(stream, request.length, metadata_limit).await?;
            let action: Request = serde_json::from_slice(&body)?;
            drop(body);
            let cancel = CancellationToken::new();
            let work = executor.execute(root, cas, action, cancel.clone());
            tokio::pin!(work);
            let mut disconnected = [0_u8; 1];
            let response = tokio::select! {
                result = &mut work => result?,
                _ = stream.read(&mut disconnected) => {
                    cancel.cancel();
                    let result = work.await;
                    if let Err(error) = result { if error.to_string().contains("cleanup") { return Err(error); } }
                    return Err(io::Error::new(io::ErrorKind::Interrupted, "worker client disconnected"));
                }
            };
            let response = serde_json::to_vec(&response)?;
            if response.len() > metadata_limit {
                return Err(invalid("response metadata limit exceeded"));
            }
            respond(stream.get_mut(), 200, &response).await
        }
        _ => Err(io::Error::new(
            io::ErrorKind::NotFound,
            "unknown worker endpoint",
        )),
    }
}

async fn pull(
    stream: &mut BufReader<TcpStream>,
    root: &std::path::Path,
    length: u64,
    metadata_limit: usize,
    cas: &mut Cas,
    executor: &Executor,
) -> io::Result<()> {
    let body = read_metadata(stream, length, metadata_limit).await?;
    let pull: super::Pull = serde_json::from_slice(&body)?;
    drop(body);
    let _permit = executor
        .resources
        .acquire_required(crate::ResourceRequest::new(
            1,
            super::shared::TRANSFER_MEMORY,
        ))
        .await
        .map_err(|_| invalid("shared transfer exceeds worker memory budget"))?;
    let missing = tokio::time::timeout(
        Duration::from_secs(300),
        super::shared::pull(&pull.cache, root, cas, &pull.digests),
    )
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "shared input transfer timed out"))??;
    let body = serde_json::to_vec(&missing)?;
    if body.len() > metadata_limit {
        return Err(invalid("CAS response metadata limit exceeded"));
    }
    respond(stream.get_mut(), 200, &body).await
}

async fn read_metadata(
    stream: &mut BufReader<TcpStream>,
    length: u64,
    limit: usize,
) -> io::Result<Vec<u8>> {
    let length = usize::try_from(length).map_err(|_| invalid("worker metadata limit exceeded"))?;
    if length > limit {
        return Err(invalid("worker metadata limit exceeded"));
    }
    let mut body = vec![0_u8; length];
    tokio::time::timeout(Duration::from_secs(30), stream.read_exact(&mut body))
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "worker metadata timeout"))??;
    Ok(body)
}

async fn header(stream: &mut TcpStream, status: u16, length: u64) -> io::Result<()> {
    let reason = match status {
        200 => "OK",
        401 => "Unauthorized",
        404 => "Not Found",
        408 => "Request Timeout",
        507 => "Insufficient Storage",
        _ => "Bad Request",
    };
    stream.write_all(format!("HTTP/1.1 {status} {reason}\r\nContent-Length: {length}\r\nConnection: close\r\nContent-Type: application/json\r\n\r\n").as_bytes()).await
}

async fn respond(stream: &mut TcpStream, status: u16, body: &[u8]) -> io::Result<()> {
    header(stream, status, body.len() as u64).await?;
    stream.write_all(body).await
}
