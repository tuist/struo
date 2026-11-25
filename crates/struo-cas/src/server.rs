//! Bounded HTTP service for opaque cache objects.

mod protocol;
mod storage;

#[cfg(test)]
mod tests;

use std::io;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, Semaphore};
use tokio::task::JoinSet;
use tokio::time::timeout;

use crate::TransferBudget;

use self::protocol::{reply, Method, Request};
use self::storage::Storage;

const CHUNK: usize = 64 * 1024;
const IDLE: Duration = Duration::from_secs(10);
const CONNECTIONS: usize = 8;

/// Serve authenticated GET, HEAD and PUT requests on an already-bound listener.
///
/// Keys are opaque `v1.cas.<64 lowercase hex>` or `v1.ac.<64 lowercase hex>`
/// names. Objects and in-progress replacements share `capacity_bytes`, charged
/// in 4 KiB units. Each of at most eight connections reserves 128 KiB from
/// `budget` before reading; uploads also reserve their rounded staging charge.
/// Idle reads, writes and admission waits time out after ten seconds.
///
/// The directory must be exclusively owned by this service. Startup removes
/// abandoned `.upload-` files and reconstructs usage from regular object files.
/// Dropping this future cancels its connections; completed objects survive a
/// restart. The token must be nonempty and is never logged.
pub async fn serve(
    listener: TcpListener,
    directory: impl AsRef<Path>,
    token: String,
    capacity_bytes: u64,
    budget: TransferBudget,
) -> io::Result<()> {
    if token.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "empty cache token",
        ));
    }
    let storage = Arc::new(Mutex::new(
        Storage::open(directory.as_ref(), capacity_bytes).await?,
    ));
    let auth = Arc::new(format!("Bearer {token}").into_bytes());
    let slots = Arc::new(Semaphore::new(CONNECTIONS));
    let mut tasks = JoinSet::new();
    loop {
        while tasks.try_join_next().is_some() {}
        tokio::select! {
            accepted = listener.accept() => {
                let (mut socket, _) = accepted?;
                let Ok(slot) = slots.clone().try_acquire_owned() else {
                    let _ = socket.shutdown().await;
                    continue;
                };
                let storage = storage.clone();
                let auth = auth.clone();
                let budget = budget.clone();
                tasks.spawn(async move {
                    let _slot = slot;
                    if let Ok(Ok(_memory)) = timeout(IDLE, budget.reserve_memory((2 * CHUNK) as u64)).await {
                        // Transport failures are expected for interrupted transfers.
                        let _ = connection(&mut socket, storage, &auth, capacity_bytes, &budget).await;
                    }
                    let _ = socket.shutdown().await;
                });
            }
            _ = tasks.join_next(), if !tasks.is_empty() => {}
        }
    }
}

async fn connection(
    socket: &mut TcpStream,
    storage: Arc<Mutex<Storage>>,
    auth: &[u8],
    capacity: u64,
    budget: &TransferBudget,
) -> io::Result<()> {
    let mut buffer = vec![0; CHUNK];
    let (request, start, end) = match protocol::read(socket, &mut buffer, auth, capacity).await {
        Ok(value) => value,
        Err(status) => return reply(socket, status, 0).await,
    };
    let Request {
        method,
        key,
        length,
        expect,
    } = request;
    let staging = if method == Method::Put {
        match timeout(IDLE, budget.reserve_staging(storage::charge(length))).await {
            Ok(Ok(reservation)) => Some(reservation),
            _ => return reply(socket, 507, 0).await,
        }
    } else {
        None
    };
    // Serializing disk transfers also pins every active reader against eviction
    // and keeps replacement accounting atomic across cancellation.
    let Ok(mut storage) = timeout(IDLE, storage.lock()).await else {
        return reply(socket, 503, 0).await;
    };
    if method == Method::Put {
        if !storage.prepare(&key, length).await? {
            return reply(socket, 507, 0).await;
        }
        let temporary = tempfile::Builder::new()
            .prefix(".upload-")
            .tempfile_in(&storage.directory)?;
        let mut destination = tokio::fs::File::from_std(temporary.reopen()?);
        if expect {
            timeout(IDLE, socket.write_all(b"HTTP/1.1 100 Continue\r\n\r\n")).await??;
        }
        let buffered = (end - start).min(usize::try_from(length).unwrap_or(usize::MAX));
        destination
            .write_all(&buffer[start..start + buffered])
            .await?;
        let mut remaining = length - buffered as u64;
        while remaining != 0 {
            let count = usize::try_from(remaining).unwrap_or(usize::MAX).min(CHUNK);
            let read = match timeout(IDLE, socket.read(&mut buffer[..count])).await {
                Ok(Ok(read)) if read != 0 => read,
                _ => return reply(socket, 400, 0).await,
            };
            destination.write_all(&buffer[..read]).await?;
            remaining -= read as u64;
        }
        destination.flush().await?;
        storage.touch(temporary.as_file())?;
        destination.sync_all().await?;
        drop(destination);
        storage.publish(temporary, &key, length)?;
        drop(staging);
        reply(socket, 204, 0).await
    } else {
        let Some((mut source, length)) = storage.reader(&key).await? else {
            return reply(socket, 404, 0).await;
        };
        reply(socket, 200, length).await?;
        if method == Method::Get {
            loop {
                let count = source.read(&mut buffer).await?;
                if count == 0 {
                    break;
                }
                timeout(IDLE, socket.write_all(&buffer[..count])).await??;
            }
        }
        Ok(())
    }
}
