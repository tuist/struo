use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::BufReader;
use tokio::net::TcpListener;

use super::super::cas::{invalid, Cas};
use super::super::execution::Executor;
use super::super::ServeOptions;
use crate::{ResourceLimits, ResourcePool};

pub(in crate::worker) async fn serve(
    listener: TcpListener,
    root: PathBuf,
    token: String,
    mut options: ServeOptions,
) -> io::Result<()> {
    if token.len() < 32 || token.len() > 4096 || token.bytes().any(|byte| !byte.is_ascii_graphic())
    {
        return Err(invalid(
            "worker bearer token must contain 32 to 4096 printable non-space bytes",
        ));
    }
    let visible_memory = ResourceLimits::default().memory_bytes.max(1);
    options.memory_bytes = if options.memory_bytes == 0 {
        visible_memory
    } else {
        options.memory_bytes.min(visible_memory)
    };
    if options.memory_bytes < 1024 * 1024
        || options.capacity_bytes == 0
        || options.environment.len() > 4096
    {
        return Err(invalid(
            "invalid worker capacity, memory budget or environment",
        ));
    }
    std::fs::create_dir_all(&root)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))?;
    }
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(root.join("service.lock"))?;
    let mut lock = fd_lock::RwLock::new(lock);
    let _exclusive = lock
        .try_write()
        .map_err(|_| invalid("worker root already in use"))?;
    for entry in std::fs::read_dir(&root)? {
        let entry = entry?;
        if entry.file_name().to_string_lossy().starts_with("action-") {
            if entry.file_type()?.is_symlink() {
                return Err(invalid("stale workspace is a symlink"));
            }
            std::fs::remove_dir_all(entry.path())?;
        }
    }
    let metadata_limit = (options.memory_bytes / 32).min(8 * 1024 * 1024) as usize;
    let overhead = (metadata_limit as u64 * 16).max(512 * 1024);
    let resources = Arc::new(ResourcePool::new(ResourceLimits::new(
        1,
        options.memory_bytes - overhead,
    )));
    let mut cas = Cas::open(root.join("cas"), options.capacity_bytes)?;
    let mut executor = Executor::new(options, resources);
    executor.probe(&root).await?;
    loop {
        let (stream, _) = listener.accept().await?;
        let mut stream = BufReader::new(stream);
        let result = super::connection(
            &mut stream,
            &root,
            &token,
            metadata_limit,
            &mut cas,
            &mut executor,
        )
        .await;
        if let Err(error) = result {
            if error.to_string().contains("cleanup") {
                return Err(error);
            }
            let status = if error.to_string().contains("capacity") {
                507
            } else if error.kind() == io::ErrorKind::NotFound {
                404
            } else if error.kind() == io::ErrorKind::PermissionDenied {
                401
            } else if error.kind() == io::ErrorKind::TimedOut {
                408
            } else {
                400
            };
            let body = if status == 507 {
                b"worker capacity exceeded".as_slice()
            } else {
                b"worker request failed".as_slice()
            };
            let _ = tokio::time::timeout(
                Duration::from_secs(5),
                super::respond(stream.get_mut(), status, body),
            )
            .await;
        }
    }
}
