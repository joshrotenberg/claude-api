//! Multi-agent session threads.
//!
//! When an agent has `callable_agents` configured, the coordinator can
//! delegate work to those sub-agents at runtime. Each delegation runs
//! in its own **thread**: a context-isolated event stream with its own
//! conversation history. The session-level event stream is the
//! "primary thread" and shows aggregated activity; per-thread streams
//! drill into one sub-agent's reasoning and tool calls.
//!
//! Threads are a Research Preview feature; the full multi-agent
//! workflow is gated on the same `managed-agents-2026-04-01` beta
//! header as the rest of Managed Agents.

use serde::{Deserialize, Serialize};

use crate::client::Client;
use crate::error::Result;
use crate::pagination::Paginated;

use super::MANAGED_AGENTS_BETA;
use super::events::SessionEvent;

/// One thread inside a multi-agent session.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Thread {
    /// Stable identifier (`sthr_...`).
    pub id: String,
    /// Wire type tag (`"session_thread"`).
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub ty: Option<String>,
    /// Name of the agent driving this thread.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_name: Option<String>,
    /// Lifecycle status. Same enum shape as the parent session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<super::sessions::SessionStatus>,
    /// Model the thread runs against.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<crate::types::ModelId>,
    /// Creation timestamp (RFC3339).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
}

/// Namespace handle for thread operations on a single session.
///
/// Obtained via [`Sessions::threads`](super::sessions::Sessions::threads).
pub struct Threads<'a> {
    pub(crate) client: &'a Client,
    pub(crate) session_id: String,
}

impl Threads<'_> {
    /// `GET /v1/sessions/{session_id}/threads`. List the threads in a
    /// session, including the primary thread (if exposed by the
    /// server) and any spawned sub-agent threads.
    pub async fn list(&self) -> Result<Paginated<Thread>> {
        let path = format!("/v1/sessions/{}/threads", self.session_id);
        self.client
            .execute_with_retry(
                || self.client.request_builder(reqwest::Method::GET, &path),
                &[MANAGED_AGENTS_BETA],
            )
            .await
    }

    /// Sub-namespace for events on a single thread.
    #[must_use]
    pub fn events(&self, thread_id: impl Into<String>) -> ThreadEvents<'_> {
        ThreadEvents {
            client: self.client,
            session_id: self.session_id.clone(),
            thread_id: thread_id.into(),
        }
    }
}

/// Namespace handle for events on a specific thread.
pub struct ThreadEvents<'a> {
    client: &'a Client,
    session_id: String,
    thread_id: String,
}

impl ThreadEvents<'_> {
    /// `GET /v1/sessions/{session_id}/threads/{thread_id}/events`.
    pub async fn list(&self) -> Result<Paginated<SessionEvent>> {
        let path = format!(
            "/v1/sessions/{}/threads/{}/events",
            self.session_id, self.thread_id
        );
        self.client
            .execute_with_retry(
                || self.client.request_builder(reqwest::Method::GET, &path),
                &[MANAGED_AGENTS_BETA],
            )
            .await
    }

    /// `GET /v1/sessions/{session_id}/threads/{thread_id}/stream`.
    /// Returns an
    /// [`EventStream`](crate::managed_agents::events::EventStream)
    /// scoped to a single thread. Events fired before the stream
    /// connects are not delivered; pair with [`Self::list`] to seed a
    /// set of seen IDs.
    #[cfg(feature = "streaming")]
    #[cfg_attr(docsrs, doc(cfg(feature = "streaming")))]
    pub async fn stream(&self) -> Result<crate::managed_agents::events::EventStream> {
        let path = format!(
            "/v1/sessions/{}/threads/{}/stream",
            self.session_id, self.thread_id
        );
        let response = self
            .client
            .execute_streaming(
                self.client
                    .request_builder(reqwest::Method::GET, &path)
                    .header("accept", "text/event-stream"),
                &[MANAGED_AGENTS_BETA],
            )
            .await?;
        Ok(crate::managed_agents::events::EventStream::from_response(
            response,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn client_for(mock: &MockServer) -> Client {
        Client::builder()
            .api_key("sk-ant-test")
            .base_url(mock.uri())
            .build()
            .unwrap()
    }

    #[tokio::test]
    async fn list_threads_returns_typed_records() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/sessions/sesn_x/threads"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [
                    {
                        "id": "sthr_a",
                        "type": "session_thread",
                        "agent_name": "Reviewer",
                        "status": "running",
                        "model": "claude-opus-4-7"
                    }
                ],
                "has_more": false
            })))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let page = client
            .managed_agents()
            .sessions()
            .threads("sesn_x")
            .list()
            .await
            .unwrap();
        assert_eq!(page.data.len(), 1);
        assert_eq!(page.data[0].agent_name.as_deref(), Some("Reviewer"));
    }

    #[tokio::test]
    async fn list_thread_events_returns_typed_session_events() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/sessions/sesn_x/threads/sthr_a/events"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [
                    {"type": "agent.message", "id": "sevt_1", "content": [{"type": "text", "text": "hi"}]}
                ],
                "has_more": false
            })))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let page = client
            .managed_agents()
            .sessions()
            .threads("sesn_x")
            .events("sthr_a")
            .list()
            .await
            .unwrap();
        assert_eq!(page.data.len(), 1);
    }

    #[cfg(feature = "streaming")]
    #[tokio::test]
    async fn stream_thread_yields_typed_events() {
        use futures_util::StreamExt;
        use wiremock::matchers::header;
        let sse = concat!(
            "event: message\n",
            "data: {\"type\":\"agent.message\",\"id\":\"sevt_1\",\"content\":[{\"type\":\"text\",\"text\":\"hi\"}]}\n",
            "\n",
            "event: message\n",
            "data: {\"type\":\"session.thread_idle\",\"id\":\"sevt_2\"}\n",
            "\n",
        );
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/sessions/sesn_x/threads/sthr_a/stream"))
            .and(header("anthropic-beta", "managed-agents-2026-04-01"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(sse),
            )
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let mut stream = client
            .managed_agents()
            .sessions()
            .threads("sesn_x")
            .events("sthr_a")
            .stream()
            .await
            .unwrap();
        let first = stream.next().await.unwrap().unwrap();
        let second = stream.next().await.unwrap().unwrap();
        assert!(first.type_tag().as_deref() == Some("agent.message"));
        assert!(second.type_tag().as_deref() == Some("session.thread_idle"));
    }
}
