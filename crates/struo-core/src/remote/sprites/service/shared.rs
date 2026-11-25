use std::path::Path;

use struo_cas::{CacheProvider, Digest, RemoteAccess};

use super::Service;
use crate::remote::{http, phase_permit, PhasePermit, PreparedCommand};
use crate::{worker::SharedCache, Result};

pub(super) async fn connection(cache: &CacheProvider) -> Option<SharedCache> {
    let CacheProvider::Remote(remote) = cache else {
        return None;
    };
    if let Ok(connection) = remote.store().http_connection().await {
        connection.map(|connection| SharedCache {
            connection,
            read_only: remote.access() == RemoteAccess::ReadOnly,
        })
    } else {
        tracing::debug!("shared worker connection unavailable; using coordinator transfers");
        None
    }
}

impl Service {
    pub(super) async fn pull(
        &self,
        cache: &SharedCache,
        digests: &[Digest],
    ) -> Result<Vec<Digest>> {
        let body = http::encode(
            &crate::worker::Pull {
                cache: cache.clone(),
                digests: digests.to_vec(),
            },
            "sprites",
            self.limits.max_metadata_bytes,
        )?;
        let response = self
            .send(
                self.request(reqwest::Method::POST, "/v1/cas/pull")?
                    .header("content-type", "application/json")
                    .body(body)
                    .timeout(self.limits.artifact_download_timeout),
            )
            .await?;
        http::decode(
            response,
            "sprites",
            self.limits.max_metadata_bytes,
            self.limits,
        )
        .await
    }

    pub(super) async fn download_shared(
        &self,
        cache: &CacheProvider,
        digest: Digest,
        path: &Path,
        command: &PreparedCommand<'_>,
    ) -> Result<Option<PhasePermit>> {
        let CacheProvider::Remote(remote) = cache else {
            return Ok(None);
        };
        let Some(size) = remote
            .store()
            .head(&struo_cas::remote::blob_key(&digest))
            .await?
        else {
            return Ok(None);
        };
        http::check_size(
            size,
            self.limits.max_archive_bytes,
            "sprites",
            "shared output bytes",
        )?;
        let phase = phase_permit(command.lease.as_ref(), size.saturating_mul(2)).await?;
        cache.copy_blob_to_file(&digest, path).await?;
        Ok(Some(phase))
    }
}
