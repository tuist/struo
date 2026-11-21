use std::time::Duration;

use serde::Deserialize;
use serde_json::json;

use crate::remote::lifecycle::{HostedSandbox, Marker};
use crate::{Error, RemoteExecution, RemoteIoLimits, ResourceRequest, Result};

#[derive(Clone)]
pub(super) struct Client {
    pub http: reqwest::Client,
    pub endpoint: reqwest::Url,
    pub token: String,
    pub project: String,
    pub team: Option<String>,
    pub plan: String,
    pub limits: RemoteIoLimits,
}

#[derive(Deserialize)]
pub(super) struct Envelope<T> {
    pub data: T,
}

#[derive(Deserialize)]
pub(super) struct Service {
    pub id: String,
    pub description: Option<String>,
    pub status: Status,
}

#[derive(Deserialize)]
pub(super) struct Status {
    pub deployment: Deployment,
}

#[derive(Deserialize)]
pub(super) struct Deployment {
    pub status: String,
}

impl HostedSandbox for Service {
    fn id(&self) -> &str {
        &self.id
    }
}

impl Client {
    pub fn from_binding(remote: &RemoteExecution) -> Result<Self> {
        let token = std::env::var("STRUO_NORTHFLANK_TOKEN")
            .or_else(|_| std::env::var("NORTHFLANK_TOKEN"))
            .map_err(|_| config_error("set STRUO_NORTHFLANK_TOKEN or NORTHFLANK_TOKEN"))?;
        if token.trim().is_empty() {
            return Err(config_error("Northflank token must not be empty"));
        }
        let project = remote
            .project
            .clone()
            .ok_or_else(|| config_error("set project on the Northflank execution binding"))?;
        let plan = remote
            .properties
            .get("deployment_plan")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                config_error("set properties.deployment_plan on the Northflank execution binding")
            })?
            .to_string();
        let endpoint = std::env::var("STRUO_NORTHFLANK_API_URL")
            .unwrap_or_else(|_| "https://api.northflank.com".into());
        Self::new(&endpoint, token, project, remote.account.clone(), plan)
    }

    pub fn new(
        endpoint: &str,
        token: String,
        project: String,
        team: Option<String>,
        plan: String,
    ) -> Result<Self> {
        let endpoint = reqwest::Url::parse(endpoint)
            .map_err(|_| config_error("invalid Northflank API URL"))?;
        if !matches!(endpoint.scheme(), "https" | "http")
            || endpoint.host_str().is_none()
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
            || endpoint.path() != "/"
        {
            return Err(config_error("Northflank API URL must be an HTTP(S) origin without credentials, path, query, or fragment"));
        }
        if project.trim().is_empty() || team.as_deref().is_some_and(|team| team.trim().is_empty()) {
            return Err(config_error(
                "Northflank project and optional account (team ID) must not be empty",
            ));
        }
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(30))
            .build()
            .map_err(http_error)?;
        Ok(Self {
            http,
            endpoint,
            token,
            project,
            team,
            plan,
            limits: RemoteIoLimits::default(),
        })
    }

    pub fn url(&self, exec: bool, service: Option<&str>) -> reqwest::Url {
        let mut url = self.endpoint.clone();
        let mut path = url
            .path_segments_mut()
            .expect("HTTP origin has path segments");
        path.clear().push("v1");
        if exec {
            path.push("command-exec");
        }
        if let Some(team) = &self.team {
            path.push("teams").push(team);
        }
        path.push("projects").push(&self.project).push("services");
        if let Some(service) = service {
            path.push(service);
        }
        drop(path);
        url
    }

    pub fn request(&self, method: reqwest::Method, url: reqwest::Url) -> reqwest::RequestBuilder {
        self.http
            .request(method, url)
            .bearer_auth(&self.token)
            .timeout(Duration::from_secs(30))
    }

    pub async fn create(&self, image: &str, expiry: u64, marker: &Marker) -> Result<Service> {
        let body = json!({
            "name": format!("struo-{}", marker.nonce),
            "description": description(&marker.nonce),
            "billing": { "deploymentPlan": self.plan },
            "deployment": {
                "instances": 1,
                "external": { "imagePath": image },
                "docker": {
                    "configType": "customEntrypointCustomCommand",
                    "customEntrypoint": "sleep",
                    "customCommand": "infinity"
                }
            },
            "createOptions": { "expiryTime": expiry }
        });
        let response = self
            .request(reqwest::Method::POST, self.url(false, Some("deployment")))
            .header("Content-Type", "application/json")
            .body(super::super::http::encode(
                &body,
                "northflank",
                self.limits.control_bytes(),
            )?)
            .send()
            .await
            .map_err(http_error)?;
        let result: Envelope<Service> = self.decode(response).await?;
        Ok(result.data)
    }

    pub async fn wait_ready(&self, mut service: Service) -> Result<Service> {
        loop {
            match service.status.deployment.status.as_str() {
                "COMPLETED" => return Ok(service),
                "FAILED" => return Err(api_error("service deployment failed")),
                "PENDING" | "IN_PROGRESS" => {}
                _ => return Err(api_error("unrecognized service deployment status")),
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
            let response = self
                .request(reqwest::Method::GET, self.url(false, Some(&service.id)))
                .send()
                .await
                .map_err(http_error)?;
            service = self.decode::<Envelope<Service>>(response).await?.data;
        }
    }

    #[allow(clippy::cast_precision_loss)]
    pub async fn validate_plan(&self, resources: &ResourceRequest) -> Result<()> {
        #[derive(Deserialize)]
        struct Plans {
            plans: Vec<Plan>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Plan {
            id: String,
            cpu_resource: f64,
            ram_resource: f64,
        }
        let response = self
            .request(
                reqwest::Method::GET,
                self.endpoint.join("v1/plans").expect("static path"),
            )
            .send()
            .await
            .map_err(http_error)?;
        let plans: Envelope<Plans> = self.decode(response).await?;
        let plan = plans
            .data
            .plans
            .iter()
            .find(|plan| plan.id == self.plan)
            .ok_or_else(|| {
                config_error("deployment_plan was not found in Northflank's available plans")
            })?;
        if plan.cpu_resource < resources.cpu_slots.max(1) as f64
            || plan.ram_resource * 1_048_576.0 < resources.memory_bytes as f64
        {
            return Err(config_error(
                "deployment_plan does not satisfy the action's CPU and memory request",
            ));
        }
        Ok(())
    }

    pub async fn decode<T: serde::de::DeserializeOwned>(
        &self,
        response: reqwest::Response,
    ) -> Result<T> {
        super::super::http::decode(
            response,
            "northflank",
            self.limits.control_bytes(),
            self.limits,
        )
        .await
    }
}

pub(super) fn description(nonce: &str) -> String {
    format!("struo-operation: {nonce}")
}

pub(super) fn config_error(message: &str) -> Error {
    Error::RemoteProviderConfig {
        provider: "northflank".into(),
        message: message.into(),
    }
}

pub(super) fn api_error(message: &str) -> Error {
    Error::RemoteProviderApi {
        provider: "northflank".into(),
        message: message.into(),
    }
}

pub(super) fn http_error(source: reqwest::Error) -> Error {
    super::super::http::http_error("northflank", source)
}
