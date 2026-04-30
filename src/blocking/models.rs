//! Synchronous Models namespace.

use crate::error::Result;
use crate::models::{ListModelsParams, ModelInfo};
use crate::pagination::Paginated;

use super::Client;

/// Namespace handle for the Models API (sync).
///
/// Obtained via [`Client::models`].
pub struct Models<'a> {
    client: &'a Client,
}

impl<'a> Models<'a> {
    pub(crate) fn new(client: &'a Client) -> Self {
        Self { client }
    }

    /// Fetch one page of models.
    #[allow(clippy::needless_pass_by_value)]
    pub fn list(&self, params: ListModelsParams) -> Result<Paginated<ModelInfo>> {
        let params_ref = &params;
        self.client.execute_with_retry(
            || {
                self.client
                    .request_builder(reqwest::Method::GET, "/v1/models")
                    .query(params_ref)
            },
            &[],
        )
    }

    /// Fetch all models, transparently paging until exhausted.
    pub fn list_all(&self) -> Result<Vec<ModelInfo>> {
        let mut all = Vec::new();
        let mut params = ListModelsParams::default();
        loop {
            let page = self.list(params.clone())?;
            let next_cursor = page.next_after().map(str::to_owned);
            all.extend(page.data);
            match next_cursor {
                Some(cursor) => params.after_id = Some(cursor),
                None => break,
            }
        }
        Ok(all)
    }

    /// Fetch metadata for a single model by ID.
    pub fn get(&self, id: impl AsRef<str>) -> Result<ModelInfo> {
        let path = format!("/v1/models/{}", id.as_ref());
        self.client.execute_with_retry(
            || self.client.request_builder(reqwest::Method::GET, &path),
            &[],
        )
    }
}
