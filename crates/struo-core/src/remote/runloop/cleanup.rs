use std::{future::Future, time::Duration};

use super::client::{lifetime, Client, Sandbox};
use crate::remote::lifecycle::{
    self, Budget, Cleanup, HostedSandbox, Journal, Lookup, Provider, Report,
};
use crate::{ExecutionPermit, Result};

impl Provider for Client {
    const NAME: &'static str = "runloop";
    fn scope(&self) -> String {
        lifecycle::scope("runloop", &self.config.url)
    }
    fn credential(&self) -> String {
        lifecycle::credential("runloop", &self.config.key)
    }
    fn find(&self, nonce: &str) -> impl Future<Output = Result<Lookup>> + Send {
        self.find_marked(nonce)
    }
    fn delete(&self, id: &str) -> impl Future<Output = Result<()>> + Send {
        self.shutdown(id)
    }
}

impl HostedSandbox for Sandbox {
    fn id(&self) -> &str {
        &self.id
    }
}

impl Client {
    pub(super) async fn create_owned(
        &self,
        journal: Journal,
        blueprint: Option<String>,
        timeout_ms: Option<u64>,
        lease: Option<ExecutionPermit>,
    ) -> Result<(Sandbox, Cleanup<Client>)> {
        let lifetime = lifetime(timeout_ms)?;
        lifecycle::create_owned(
            self.clone(),
            Some(journal),
            lifetime.saturating_add(Duration::from_mins(10)),
            lease,
            move |client, marker| async move {
                client
                    .create_marked(blueprint.as_deref(), lifetime, marker.as_ref())
                    .await
            },
        )
        .await
    }

    pub(super) async fn reconcile(&self, journal: &Journal) -> Report {
        lifecycle::reconcile(journal, self, Budget::default()).await
    }
}
