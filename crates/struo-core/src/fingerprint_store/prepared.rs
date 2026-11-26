use std::sync::Arc;

use anyhow::{ensure, Result};
use serde_json::Value;
use struo_cas::Digest;

use crate::{InputFingerprintManifest, ResourcePermit, ResourcePool};

const MAX_BYTES: usize = 64 * 1024;

pub(super) struct Prepared {
    bytes: Vec<u8>,
    stored: Value,
    _permit: ResourcePermit,
}

impl Prepared {
    pub(super) fn new(
        manifest: &InputFingerprintManifest,
        resources: &Arc<ResourcePool>,
    ) -> Option<Self> {
        if manifest.components.is_empty() {
            return None;
        }
        // Worst-case JSON escaping plus a block, branch and framing per component.
        let mut bound = manifest.schema.len().checked_add(1024)?;
        for component in &manifest.components {
            bound = bound.checked_add(
                component
                    .category
                    .len()
                    .checked_add(component.label.len())?
                    .checked_mul(6)?
                    .checked_add(512)?,
            )?;
            if bound > MAX_BYTES {
                return None;
            }
        }
        // Include serializer buffers and tree spine growth, not just retained bytes.
        let permit = resources.try_reserve_memory(u64::try_from(bound.checked_mul(8)?).ok()?)?;
        let mut bytes = Vec::with_capacity(bound);
        let stored = super::encode_stored(manifest, |digest, node| {
            ensure!(
                bytes.len() + Digest::LEN + 4 + node.len() <= bound,
                "prepared fingerprint exceeds reservation"
            );
            bytes.extend_from_slice(digest.as_bytes());
            bytes.extend_from_slice(&u32::try_from(node.len())?.to_le_bytes());
            bytes.extend_from_slice(node);
            Ok(())
        })
        .ok()?;
        Some(Self {
            bytes,
            stored,
            _permit: permit,
        })
    }

    pub(super) fn write(self, mut write: impl FnMut(Digest, &[u8]) -> Result<()>) -> Result<Value> {
        let mut remaining = self.bytes.as_slice();
        while !remaining.is_empty() {
            let (digest, rest) = remaining.split_at(Digest::LEN);
            let (size, rest) = rest.split_at(4);
            let size = u32::from_le_bytes(size.try_into().expect("prepared length")) as usize;
            let (node, rest) = rest.split_at(size);
            write(
                Digest::from_bytes(digest.try_into().expect("prepared digest")),
                node,
            )?;
            remaining = rest;
        }
        Ok(self.stored)
    }
}
