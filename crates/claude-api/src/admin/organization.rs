//! `GET /v1/organizations/me`.

use serde::{Deserialize, Serialize};

use crate::client::Client;
use crate::error::Result;

/// The authenticated organization.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct OrganizationInfo {
    /// Stable organization ID.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Wire type tag (always `"organization"`).
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub ty: Option<String>,
}

/// Namespace handle for the organization endpoint.
pub struct Organization<'a> {
    client: &'a Client,
}

impl<'a> Organization<'a> {
    pub(crate) fn new(client: &'a Client) -> Self {
        Self { client }
    }

    /// `GET /v1/organizations/me`.
    pub async fn me(&self) -> Result<OrganizationInfo> {
        self.client
            .execute_with_retry(
                || {
                    self.client
                        .request_builder(reqwest::Method::GET, "/v1/organizations/me")
                },
                &[],
            )
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn client_for(mock: &MockServer) -> Client {
        Client::builder()
            .api_key("sk-ant-admin-test")
            .base_url(mock.uri())
            .build()
            .unwrap()
    }

    #[tokio::test]
    async fn me_returns_typed_organization() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/organizations/me"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "org_01",
                "type": "organization",
                "name": "Acme"
            })))
            .mount(&mock)
            .await;
        let client = client_for(&mock);
        let org = client.admin().organization().me().await.unwrap();
        assert_eq!(org.id, "org_01");
        assert_eq!(org.name, "Acme");
    }
}
