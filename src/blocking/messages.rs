//! Synchronous Messages namespace.

use crate::error::Result;
use crate::messages::request::{CountTokensRequest, CreateMessageRequest};
use crate::messages::response::{CountTokensResponse, Message};

use super::Client;

/// Namespace handle for the Messages API (sync).
///
/// Obtained via [`Client::messages`].
pub struct Messages<'a> {
    client: &'a Client,
}

impl<'a> Messages<'a> {
    pub(crate) fn new(client: &'a Client) -> Self {
        Self { client }
    }

    /// `POST /v1/messages`. Retries are governed by the client's
    /// [`RetryPolicy`](crate::retry::RetryPolicy).
    pub fn create(&self, request: CreateMessageRequest) -> Result<Message> {
        self.create_with_beta(request, &[])
    }

    /// Like [`Self::create`] but with per-request beta headers.
    // By-value matches the async signature; the closure only borrows.
    #[allow(clippy::needless_pass_by_value)]
    pub fn create_with_beta(
        &self,
        request: CreateMessageRequest,
        betas: &[&str],
    ) -> Result<Message> {
        let request_ref = &request;
        self.client.execute_with_retry(
            || {
                self.client
                    .request_builder(reqwest::Method::POST, "/v1/messages")
                    .json(request_ref)
            },
            betas,
        )
    }

    /// `POST /v1/messages/count_tokens`.
    pub fn count_tokens(&self, request: CountTokensRequest) -> Result<CountTokensResponse> {
        self.count_tokens_with_beta(request, &[])
    }

    /// Like [`Self::count_tokens`] but with per-request beta headers.
    #[allow(clippy::needless_pass_by_value)]
    pub fn count_tokens_with_beta(
        &self,
        request: CountTokensRequest,
        betas: &[&str],
    ) -> Result<CountTokensResponse> {
        let request_ref = &request;
        self.client.execute_with_retry(
            || {
                self.client
                    .request_builder(reqwest::Method::POST, "/v1/messages/count_tokens")
                    .json(request_ref)
            },
            betas,
        )
    }
}
