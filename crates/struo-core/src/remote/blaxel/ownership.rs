use std::future::Future;
use std::time::Duration;

use super::client::{Client, Sandbox};
use super::control::lifetime;
use crate::remote::lifecycle::{
    self, Budget, Cleanup, HostedSandbox, Journal, Lookup, Provider, Report,
};
use crate::{ExecutionPermit, ResourceRequest, Result};

impl Provider for Client {
    const NAME: &'static str = "blaxel";
    fn scope(&self) -> String {
        Client::scope(self)
    }
    fn credential(&self) -> String {
        Client::credential(self)
    }
    fn find(&self, nonce: &str) -> impl Future<Output = Result<Lookup>> + Send {
        self.find_marked(nonce)
    }
    fn delete(&self, id: &str) -> impl Future<Output = Result<()>> + Send {
        Client::delete(self, id)
    }
}

impl HostedSandbox for Sandbox {
    fn id(&self) -> &str {
        &self.metadata.name
    }
}

impl Client {
    pub(super) async fn create_owned(
        &self,
        journal: Journal,
        image: Option<String>,
        resources: ResourceRequest,
        timeout_ms: Option<u64>,
        lease: Option<ExecutionPermit>,
    ) -> Result<(Sandbox, Cleanup<Client>)> {
        lifecycle::create_owned(
            self.clone(),
            Some(journal),
            lifetime(timeout_ms).saturating_add(Duration::from_mins(10)),
            lease,
            move |client, marker| async move {
                client
                    .create_marked(
                        image.as_deref(),
                        &resources,
                        timeout_ms,
                        &marker.expect("journal supplies a marker"),
                    )
                    .await
            },
        )
        .await
    }
    pub(super) async fn reconcile(&self, journal: &Journal) -> Report {
        lifecycle::reconcile(journal, self, Budget::default()).await
    }
}
