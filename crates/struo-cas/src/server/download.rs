use std::io;

use async_compression::codecs::zstd::params::CParameter;
use async_compression::tokio::bufread::ZstdEncoder;
use async_compression::Level;
use futures::FutureExt;
use tokio::fs::File;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::time::timeout;

use super::{protocol::reply, CHUNK, IDLE};
use crate::TransferBudget;

const ENCODER_MEMORY: u64 = 32 * 1024 * 1024;

pub(super) async fn send(
    socket: &mut TcpStream,
    source: File,
    length: u64,
    zstd: bool,
    keep_alive: bool,
    budget: &TransferBudget,
) -> io::Result<()> {
    // Idle keep-alive sockets hold memory; never wait for them to free it.
    let memory = if zstd && length >= CHUNK as u64 {
        budget
            .reserve_memory(ENCODER_MEMORY)
            .now_or_never()
            .and_then(Result::ok)
    } else {
        None
    };
    let compressed = memory.is_some();
    let mut source: std::pin::Pin<Box<dyn AsyncRead + Send>> = if compressed {
        let connection = if keep_alive { "keep-alive" } else { "close" };
        let header = format!(
            "HTTP/1.1 200 OK\r\nContent-Encoding: zstd\r\nTransfer-Encoding: chunked\r\nX-Struo-Uncompressed-Length: {length}\r\nVary: Accept-Encoding\r\nConnection: {connection}\r\n\r\n"
        );
        timeout(IDLE, socket.write_all(header.as_bytes())).await??;
        Box::pin(ZstdEncoder::with_quality_and_params(
            BufReader::with_capacity(CHUNK, source),
            Level::Precise(3),
            &[CParameter::window_log(21)],
        ))
    } else {
        reply(socket, 200, length, keep_alive).await?;
        Box::pin(source)
    };
    let mut buffer = vec![0; CHUNK];
    loop {
        let count = source.read(&mut buffer).await?;
        if count == 0 {
            break;
        }
        if compressed {
            timeout(IDLE, socket.write_all(format!("{count:x}\r\n").as_bytes())).await??;
        }
        timeout(IDLE, socket.write_all(&buffer[..count])).await??;
        if compressed {
            timeout(IDLE, socket.write_all(b"\r\n")).await??;
        }
    }
    if compressed {
        timeout(IDLE, socket.write_all(b"0\r\n\r\n")).await??;
    }
    Ok(())
}
