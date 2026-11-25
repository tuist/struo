use std::collections::HashSet;
use std::io;
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use struo_cas::{remote::blob_key, Digest, HttpCacheRemote, RemoteStore};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

use super::cas::{invalid, Cas};
use super::{Input, InputKind, Response, SharedCache};

pub(super) const TRANSFER_MEMORY: u64 = struo_cas::MIN_TRANSFER_MEMORY_BYTES;
const TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Pull {
    pub cache: SharedCache,
    pub digests: Vec<Digest>,
}

pub(super) async fn pull(
    cache: &SharedCache,
    root: &Path,
    cas: &mut Cas,
    digests: &[Digest],
) -> io::Result<Vec<Digest>> {
    let store = cache.connection.connect().map_err(io::Error::other)?;
    let pinned: HashSet<_> = digests.iter().copied().collect();
    for digest in &pinned {
        if cas.path(digest).is_file() {
            continue;
        }
        let fetch = async {
            let Some(body) = store
                .get(&blob_key(digest))
                .await
                .map_err(io::Error::other)?
            else {
                return Ok(());
            };
            cas.make_room_pinned(body.size, &pinned)?;
            let temporary = tempfile::NamedTempFile::new_in(root)?;
            let mut file = tokio::fs::File::from_std(temporary.reopen()?);
            let received = tokio::io::copy(
                &mut body.reader.take(body.size.saturating_add(1)),
                &mut file,
            )
            .await?;
            file.flush().await?;
            if received != body.size {
                return Err(invalid("shared blob length mismatch"));
            }
            cas.publish(temporary.path(), Some(*digest))?;
            Ok::<_, io::Error>(())
        };
        if !matches!(tokio::time::timeout(TIMEOUT, fetch).await, Ok(Ok(()))) {
            tracing::debug!(blob_digest = %digest, "shared input unavailable; coordinator fallback required");
        }
    }
    Ok(digests
        .iter()
        .copied()
        .filter(|digest| !cas.path(digest).is_file())
        .collect())
}

pub(super) async fn publish(
    cache: &SharedCache,
    workspace: &Path,
    cas: &Cas,
    inputs: &[Input],
    outputs: &[crate::WorkspacePath],
    response: &mut Response,
    cancel: &CancellationToken,
) -> io::Result<()> {
    if cache.read_only {
        return Ok(());
    }
    let store = cache.connection.connect().map_err(io::Error::other)?;
    let work = async {
        let mut count = 0;
        for output in crate::remote::output_roots(outputs) {
            publish_tree(&store, workspace, Path::new(output.as_str()), &mut count).await?;
        }
        for input in inputs {
            if let InputKind::File { digest, .. } = &input.kind {
                publish_file(&store, &cas.path(digest), *digest).await?;
            }
        }
        for digest in [
            response.output_archive,
            response.result.stdout,
            response.result.stderr,
        ]
        .into_iter()
        .flatten()
        {
            publish_file(&store, &cas.path(&digest), digest).await?;
            response.shared_blobs.push(digest);
        }
        Ok(())
    };
    tokio::select! {
        () = cancel.cancelled() => Err(io::Error::new(io::ErrorKind::Interrupted, "shared publication cancelled")),
        result = tokio::time::timeout(TIMEOUT, work) => result.map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "shared publication timed out"))?,
    }
}

async fn publish_tree(
    store: &HttpCacheRemote,
    workspace: &Path,
    relative: &Path,
    count: &mut usize,
) -> io::Result<()> {
    *count += 1;
    if *count > 100_000 || relative.components().count() > 128 {
        return Err(invalid("shared output tree exceeds metadata limit"));
    }
    let path = crate::WorkspacePath::try_from(
        relative
            .to_str()
            .ok_or_else(|| invalid("shared output path must be UTF-8"))?,
    )
    .map_err(|_| invalid("invalid shared output path"))?;
    let source = super::sandbox::checked_path(workspace, &path, true)?;
    let metadata = match source.symlink_metadata() {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if metadata.is_file() {
        let digest = Digest::of_reader(std::fs::File::open(&source)?)?;
        publish_file(store, &source, digest).await?;
    } else if metadata.is_dir() {
        for entry in std::fs::read_dir(source)? {
            Box::pin(publish_tree(
                store,
                workspace,
                &relative.join(entry?.file_name()),
                count,
            ))
            .await?;
        }
    }
    Ok(())
}

async fn publish_file(store: &HttpCacheRemote, path: &Path, digest: Digest) -> io::Result<()> {
    let size = path.metadata()?.len();
    let key = blob_key(&digest);
    if store.head(&key).await.map_err(io::Error::other)? != Some(size) {
        store
            .put_file(&key, path, size)
            .await
            .map_err(io::Error::other)?;
    }
    Ok(())
}
