use std::io;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::storage::invalid;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::remote) struct Record {
    pub environment: String,
    pub nonce: String,
    pub provider_identity: Option<String>,
    pub checkpoint: Option<String>,
    #[serde(default)]
    pub service: Option<String>,
    pub created_at: u64,
    pub dirty: bool,
}

impl Record {
    pub(super) fn new(environment: &str) -> io::Result<Self> {
        validate_text(environment)?;
        Ok(Self {
            environment: environment.to_owned(),
            nonce: uuid::Uuid::now_v7().simple().to_string(),
            provider_identity: None,
            checkpoint: None,
            service: None,
            created_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(io::Error::other)?
                .as_secs(),
            dirty: true,
        })
    }

    pub(super) fn validate(&self) -> io::Result<()> {
        validate_text(&self.environment)?;
        if self.nonce.len() != 32
            || !self.nonce.bytes().all(|byte| byte.is_ascii_hexdigit())
            || self.created_at == 0
            || (!self.dirty && self.provider_identity.is_none())
        {
            return Err(invalid("invalid worker record"));
        }
        for text in [&self.provider_identity, &self.checkpoint, &self.service]
            .into_iter()
            .flatten()
        {
            validate_text(text)?;
        }
        Ok(())
    }
}

pub(super) fn validate_text(text: &str) -> io::Result<()> {
    if text.is_empty() || text.len() > 4096 || text.chars().any(char::is_control) {
        return Err(invalid("invalid worker record string"));
    }
    Ok(())
}
