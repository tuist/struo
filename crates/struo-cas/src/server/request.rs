use std::io;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::time::timeout;

use super::protocol::{reply, Method, Request};
use super::{storage, Storage, TransferBudget, CHUNK, IDLE};

pub(super) async fn serve(
    socket: &mut TcpStream,
    storage: &Mutex<Storage>,
    request: Request,
    input: &[u8],
    budget: &TransferBudget,
) -> io::Result<()> {
    let Request {
        method,
        key,
        length,
        expect,
        keep_alive,
    } = request;
    let mut buffer = vec![0; CHUNK];
    let staging = if method == Method::Put {
        let Ok(Ok(reservation)) =
            timeout(IDLE, budget.reserve_staging(storage::charge(length))).await
        else {
            reply(socket, 507, 0, false).await?;
            return Err(io::Error::other("upload staging budget exhausted"));
        };
        Some(reservation)
    } else {
        None
    };
    // Serializing disk transfers also pins every active reader against eviction
    // and keeps replacement accounting atomic across cancellation.
    let Ok(mut storage) = timeout(IDLE, storage.lock()).await else {
        reply(socket, 503, 0, false).await?;
        return Err(io::Error::other("cache storage busy"));
    };
    if method == Method::Put {
        if !storage.prepare(&key, length).await? {
            reply(socket, 507, 0, false).await?;
            return Err(io::Error::other("cache capacity exhausted"));
        }
        let temporary = tempfile::Builder::new()
            .prefix(".upload-")
            .tempfile_in(&storage.directory)?;
        let mut destination = tokio::fs::File::from_std(temporary.reopen()?);
        if expect {
            timeout(IDLE, socket.write_all(b"HTTP/1.1 100 Continue\r\n\r\n")).await??;
        }
        let buffered = input
            .len()
            .min(usize::try_from(length).unwrap_or(usize::MAX));
        destination.write_all(&input[..buffered]).await?;
        let mut remaining = length - buffered as u64;
        while remaining != 0 {
            let count = usize::try_from(remaining).unwrap_or(usize::MAX).min(CHUNK);
            let read = match timeout(IDLE, socket.read(&mut buffer[..count])).await {
                Ok(Ok(read)) if read != 0 => read,
                _ => {
                    reply(socket, 400, 0, false).await?;
                    return Err(io::Error::other("incomplete upload"));
                }
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
        reply(socket, 204, 0, keep_alive).await
    } else {
        let Some((mut source, length)) = storage.reader(&key).await? else {
            return reply(socket, 404, 0, keep_alive).await;
        };
        reply(socket, 200, length, keep_alive).await?;
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
