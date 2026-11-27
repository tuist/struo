//! Bounded HTTP service for opaque cache objects.

mod protocol;
mod request;
mod storage;

#[cfg(test)]
mod tests;

use std::io;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, Semaphore};
use tokio::task::JoinSet;
use tokio::time::timeout;

use crate::TransferBudget;

use self::protocol::reply;
use self::storage::Storage;

const CHUNK: usize = 64 * 1024;
const IDLE: Duration = Duration::from_secs(10);
const CONNECTION_MEMORY: u64 = (2 * CHUNK) as u64;

/// Serve authenticated GET, HEAD and PUT requests on an already-bound listener.
///
/// Keys are opaque `v1.cas.<64 lowercase hex>` or `v1.ac.<64 lowercase hex>`
/// names. Objects and in-progress replacements share `capacity_bytes`, charged
/// in 4 KiB units. Each connection reserves 128 KiB from `budget` before
/// reading; uploads also reserve their rounded staging charge. The memory
/// budget bounds the number of connections, including idle keep-alive sockets.
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
    let connections = usize::try_from(budget.memory_bytes() / CONNECTION_MEMORY)
        .unwrap_or(Semaphore::MAX_PERMITS)
        .clamp(1, Semaphore::MAX_PERMITS);
    let slots = Arc::new(Semaphore::new(connections));
    let mut tasks = JoinSet::new();
    loop {
        while tasks.try_join_next().is_some() {}
        tokio::select! {
            accepted = listener.accept() => {
                let (mut socket, _) = accepted?;
                socket.set_nodelay(true)?;
                let Ok(slot) = slots.clone().try_acquire_owned() else {
                    let _ = socket.shutdown().await;
                    continue;
                };
                let storage = storage.clone();
                let auth = auth.clone();
                let budget = budget.clone();
                tasks.spawn(async move {
                    let _slot = slot;
                    if let Ok(Ok(_memory)) = timeout(IDLE, budget.reserve_memory(CONNECTION_MEMORY)).await {
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
    let mut end = 0;
    loop {
        let (request, start, read_end) =
            match protocol::read(socket, &mut buffer, auth, capacity, end).await {
                Ok(Some(value)) => value,
                Ok(None) => return Ok(()),
                Err(status) => return reply(socket, status, 0, false).await,
            };
        let keep_alive = request.keep_alive;
        let consumed = start
            + usize::try_from(request.length)
                .unwrap_or(usize::MAX)
                .min(read_end - start);
        request::serve(socket, &storage, request, &buffer[start..read_end], budget).await?;
        if !keep_alive {
            return Ok(());
        }
        buffer.copy_within(consumed..read_end, 0);
        end = read_end - consumed;
    }
}
