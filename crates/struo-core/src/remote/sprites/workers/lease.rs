use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::super::client::{api_error, Client, Sprite};
use crate::remote::lifecycle::{HostedSandbox, Lookup, Provider};
use crate::remote::worker_pool::{Record, Slot};
use crate::{ExecutionPermit, Result, WorkerPoolSpec};

pub(super) struct Worker {
    pub(super) client: Client,
    pub(super) slot: Option<Slot>,
    pub(super) permit: Option<ExecutionPermit>,
}

impl Worker {
    pub(super) fn record(&self) -> &Record {
        self.slot.as_ref().unwrap().record().unwrap()
    }

    pub(super) fn sprite(&self) -> Result<Sprite> {
        Sprite::from_identity(self.record().provider_identity.as_deref().ok_or_else(|| api_error("worker creation outcome is uncertain; reconcile the retained worker slot before retrying"))?)
    }

    pub(super) async fn acquire(&mut self, spec: &WorkerPoolSpec, fingerprint: &str) -> Result<()> {
        let slot = self.slot.as_mut().unwrap();
        if let Some(record) = slot.record() {
            let age = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
                .saturating_sub(record.created_at);
            if !record.dirty
                && record.service.is_some()
                && record.checkpoint.is_some()
                && record.environment == fingerprint
                && age < spec.max_age_seconds
            {
                slot.record_mut().unwrap().dirty = true;
                return slot.save();
            }
        }
        if let Some(record) = slot.record() {
            let identities: Vec<String> = record.provider_identity.iter().cloned().collect();
            match self
                .client
                .find_owned(&record.nonce, &identities, &self.client.credential())
                .await?
            {
                Lookup::Absent => slot.reset()?,
                Lookup::Inconclusive => {
                    return Err(api_error(
                        "retained worker ownership is uncertain; refusing reuse or replacement",
                    ))
                }
                Lookup::Found(_) => {
                    let age = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs()
                        .saturating_sub(record.created_at);
                    if record.environment != fingerprint
                        || age >= spec.max_age_seconds
                        || record.checkpoint.is_none()
                    {
                        self.client
                            .delete(record.provider_identity.as_ref().unwrap())
                            .await?;
                        slot.reset()?;
                    } else if let Some(checkpoint) =
                        record.checkpoint.as_ref().filter(|_| record.dirty)
                    {
                        let sprite =
                            Sprite::from_identity(record.provider_identity.as_ref().unwrap())?;
                        self.client.restore(&sprite, checkpoint).await?;
                    }
                }
            }
        }
        if slot.record().is_none() {
            slot.begin(fingerprint)?;
            let sprite = self.client.create(&slot.record().unwrap().nonce).await?;
            slot.record_mut().unwrap().provider_identity = Some(sprite.id().into());
            slot.save()?;
        }
        slot.record_mut().unwrap().dirty = true;
        slot.save()
    }

    pub(super) async fn finish(mut self) -> Result<()> {
        let result = tokio::time::timeout(Duration::from_secs(150), self.clean())
            .await
            .map_err(|_| api_error("prepared worker reset timed out; slot remains dirty"))
            .and_then(std::convert::identity);
        // A failed reset remains journaled and cannot be reused without recovery.
        self.slot.take();
        result
    }

    pub(super) fn release(mut self) -> Result<()> {
        let slot = self.slot.as_mut().unwrap();
        slot.record_mut().unwrap().dirty = false;
        let result = slot.save();
        self.slot.take();
        result
    }

    async fn clean(&mut self) -> Result<()> {
        let slot = self.slot.as_mut().unwrap();
        let Some(record) = slot.record() else {
            return Ok(());
        };
        let Some(identity) = &record.provider_identity else {
            return Err(api_error(
                "worker creation outcome is uncertain; retained slot requires reconciliation",
            ));
        };
        let identities = [identity.clone()];
        match self
            .client
            .find_owned(&record.nonce, &identities, &self.client.credential())
            .await?
        {
            Lookup::Absent => return slot.reset(),
            Lookup::Inconclusive => {
                return Err(api_error("worker identity changed; refusing reset"))
            }
            Lookup::Found(_) => {}
        }
        if let Some(checkpoint) = &record.checkpoint {
            self.client
                .restore(&Sprite::from_identity(identity)?, checkpoint)
                .await?;
            slot.record_mut().unwrap().dirty = false;
            slot.save()
        } else {
            self.client.delete(identity).await?;
            slot.reset()
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        if let Some(slot) = self.slot.take() {
            let mut worker = Self {
                client: self.client.clone(),
                slot: Some(slot),
                permit: self.permit.take(),
            };
            if let Ok(runtime) = tokio::runtime::Handle::try_current() {
                runtime.spawn(async move {
                    let result = tokio::time::timeout(Duration::from_secs(150), worker.clean()).await
                        .map_err(|_| api_error("cancelled worker reset timed out")).and_then(std::convert::identity);
                    if let Err(error) = result {
                        tracing::warn!(provider = "sprites", %error, "cancelled worker reset timed out");
                    }
                    worker.slot.take();
                });
            } else {
                worker.slot.take();
            }
        }
    }
}
