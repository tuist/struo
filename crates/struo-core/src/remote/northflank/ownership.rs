use std::time::Duration;

use super::client::{api_error, description, http_error, Client, Envelope, Service};
use crate::remote::lifecycle::{self, Lookup, Provider};
use crate::Result;

impl Provider for Client {
    const NAME: &'static str = "northflank";

    fn scope(&self) -> String {
        lifecycle::scope(Self::NAME, self.url(false, None).as_str())
    }

    fn credential(&self) -> String {
        lifecycle::credential(Self::NAME, &self.token)
    }

    async fn find(&self, nonce: &str) -> Result<Lookup> {
        // The preassigned name avoids a project-wide list. The description
        // also has to match before recovery may delete the service.
        let name = format!("struo-{nonce}");
        let response = self
            .request(reqwest::Method::GET, self.url(false, Some(&name)))
            .send()
            .await
            .map_err(http_error)?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(Lookup::Absent);
        }
        let service: Envelope<Service> = self.decode(response).await?;
        if service.data.id == name
            && service.data.description.as_deref() == Some(&description(nonce))
        {
            Ok(Lookup::Found(vec![name]))
        } else {
            Ok(Lookup::Inconclusive)
        }
    }

    async fn delete(&self, id: &str) -> Result<()> {
        tokio::time::timeout(Duration::from_secs(30), async {
            let response = self
                .request(reqwest::Method::DELETE, self.url(false, Some(id)))
                .send()
                .await
                .map_err(http_error)?;
            if response.status() == reqwest::StatusCode::NOT_FOUND {
                return Ok(());
            }
            super::super::http::checked(response, "northflank")?;
            // Hold ownership and quota until asynchronous deletion settles.
            loop {
                let response = self
                    .request(reqwest::Method::GET, self.url(false, Some(id)))
                    .send()
                    .await
                    .map_err(http_error)?;
                if response.status() == reqwest::StatusCode::NOT_FOUND {
                    return Ok(());
                }
                super::super::http::checked(response, "northflank")?;
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        })
        .await
        .map_err(|_| api_error("service deletion did not settle; ownership journal retained"))?
    }
}
