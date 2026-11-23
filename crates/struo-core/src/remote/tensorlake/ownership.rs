use super::client::{api_error, Client, Identity};
use crate::remote::{
    http,
    lifecycle::{self, Lookup, Provider},
};
use crate::Result;

impl Provider for Client {
    const NAME: &'static str = "tensorlake";

    fn scope(&self) -> String {
        lifecycle::scope(
            "tensorlake",
            &format!("{}\n{}", self.endpoint, self.namespace),
        )
    }

    fn credential(&self) -> String {
        lifecycle::credential("tensorlake", &self.token)
    }

    fn find(&self, _nonce: &str) -> impl std::future::Future<Output = Result<Lookup>> + Send {
        std::future::ready(Ok(Lookup::Inconclusive))
    }

    async fn find_owned(&self, nonce: &str, ids: &[String], credential: &str) -> Result<Lookup> {
        if credential != self.credential() || ids.len() != 1 {
            return Ok(Lookup::Inconclusive);
        }
        let identity: Identity =
            serde_json::from_str(&ids[0]).map_err(|_| api_error("invalid journaled identity"))?;
        if identity.name != format!("struo-{nonce}") || identity.namespace != self.namespace {
            return Ok(Lookup::Inconclusive);
        }
        let Some(info) = self.info(&identity.id).await? else {
            return Ok(Lookup::Absent);
        };
        self.verify(&identity, &info)?;
        Ok(if info.status == "terminated" {
            Lookup::Absent
        } else {
            Lookup::Found(ids.to_vec())
        })
    }

    async fn delete(&self, value: &str) -> Result<()> {
        tokio::time::timeout(std::time::Duration::from_mins(1), self.terminate(value))
            .await
            .map_err(|_| crate::Error::Timeout(std::time::Duration::from_mins(1)))?
    }
}

impl Client {
    async fn terminate(&self, value: &str) -> Result<()> {
        let identity: Identity =
            serde_json::from_str(value).map_err(|_| api_error("invalid ownership identity"))?;
        if identity.namespace != self.namespace {
            return Err(api_error("ownership namespace mismatch"));
        }
        let Some(info) = self.info(&identity.id).await? else {
            return Ok(());
        };
        self.verify(&identity, &info)?;
        if info.status == "terminated" {
            return Ok(());
        }
        let response = self
            .control(reqwest::Method::DELETE, Some(&identity.id))?
            .send()
            .await
            .map_err(|error| http::http_error("tensorlake", error))?;
        if response.status() != reqwest::StatusCode::NOT_FOUND {
            http::checked(response, "tensorlake")?;
        }
        loop {
            let Some(info) = self.info(&identity.id).await? else {
                return Ok(());
            };
            self.verify(&identity, &info)?;
            if info.status == "terminated" {
                return Ok(());
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
    }
}
