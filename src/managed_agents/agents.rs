//! Agents (preview, create-only).
//!
//! An agent defines how Claude behaves within a session: model, system
//! prompt, MCP servers, and toolset. Sessions reference agents by ID.
//!
//! **This module currently exposes `create` only.** The docs reference
//! agent versioning ("Agents are versioned resources") and a versioned
//! [`AgentRef`](super::sessions::AgentRef) shape, but the full
//! retrieve / list / update / delete / version-history surface isn't
//! documented yet. Those land in a follow-up release once we have
//! authoritative shapes; for the meantime, build the `create` payload
//! by passing raw JSON for any tools or MCP-server config that exceeds
//! the basic shapes typed below.

use serde::{Deserialize, Serialize};

use crate::client::Client;
use crate::error::Result;
use crate::types::ModelId;

use super::MANAGED_AGENTS_BETA;

// =====================================================================
// Agent types
// =====================================================================

/// An MCP server reference on an agent definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum AgentMcpServer {
    /// HTTP(S)-reachable MCP server.
    Url {
        /// Server name; referenced by tool entries.
        name: String,
        /// Server URL.
        url: String,
    },
}

impl AgentMcpServer {
    /// Build a URL-typed MCP server reference.
    #[must_use]
    pub fn url(name: impl Into<String>, url: impl Into<String>) -> Self {
        Self::Url {
            name: name.into(),
            url: url.into(),
        }
    }
}

/// One tool entry on an agent definition.
///
/// The pre-built agent toolset (file ops, bash, etc.) is enabled via
/// [`Self::AgentToolset20260401`]. MCP tools are exposed via
/// [`Self::McpToolset`] referencing a server by name. Custom tools and
/// other variants land as full docs become available; until then, use
/// [`Self::Other`] to pass raw JSON through.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum AgentTool {
    /// Pre-built `agent_toolset_20260401`. Carries no fields beyond
    /// the type tag.
    AgentToolset20260401 {
        /// Always `"agent_toolset_20260401"`.
        #[serde(rename = "type")]
        ty: AgentToolsetTag,
    },
    /// `mcp_toolset` referencing a named MCP server.
    McpToolset {
        /// Always `"mcp_toolset"`.
        #[serde(rename = "type")]
        ty: McpToolsetTag,
        /// Server name from the agent's `mcp_servers` list.
        mcp_server_name: String,
    },
    /// Pass-through for tool shapes this SDK doesn't model yet.
    Other(serde_json::Value),
}

/// Type-tag witness for [`AgentTool::AgentToolset20260401`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum AgentToolsetTag {
    /// `"agent_toolset_20260401"`.
    #[serde(rename = "agent_toolset_20260401")]
    AgentToolset20260401,
}

/// Type-tag witness for [`AgentTool::McpToolset`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum McpToolsetTag {
    /// `"mcp_toolset"`.
    #[serde(rename = "mcp_toolset")]
    McpToolset,
}

impl AgentTool {
    /// Enable the pre-built agent toolset (bash, file ops, etc.).
    #[must_use]
    pub fn agent_toolset() -> Self {
        Self::AgentToolset20260401 {
            ty: AgentToolsetTag::AgentToolset20260401,
        }
    }

    /// Expose the named MCP server's tools to the agent.
    #[must_use]
    pub fn mcp_toolset(server_name: impl Into<String>) -> Self {
        Self::McpToolset {
            ty: McpToolsetTag::McpToolset,
            mcp_server_name: server_name.into(),
        }
    }
}

/// Request body for `POST /v1/agents`.
#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct CreateAgentRequest {
    /// Agent name.
    pub name: String,
    /// Model the agent runs against.
    pub model: ModelId,
    /// Optional system prompt.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    /// MCP servers exposed to the agent.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub mcp_servers: Vec<AgentMcpServer>,
    /// Tools the agent can invoke.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<AgentTool>,
}

impl CreateAgentRequest {
    /// Begin configuring a request.
    #[must_use]
    pub fn builder() -> CreateAgentRequestBuilder {
        CreateAgentRequestBuilder::default()
    }
}

/// Builder for [`CreateAgentRequest`].
#[derive(Debug, Default)]
pub struct CreateAgentRequestBuilder {
    name: Option<String>,
    model: Option<ModelId>,
    system: Option<String>,
    mcp_servers: Vec<AgentMcpServer>,
    tools: Vec<AgentTool>,
}

impl CreateAgentRequestBuilder {
    /// Set the agent name. Required.
    #[must_use]
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Set the model. Required.
    #[must_use]
    pub fn model(mut self, model: impl Into<ModelId>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Set the system prompt.
    #[must_use]
    pub fn system(mut self, system: impl Into<String>) -> Self {
        self.system = Some(system.into());
        self
    }

    /// Append an MCP server reference.
    #[must_use]
    pub fn mcp_server(mut self, server: AgentMcpServer) -> Self {
        self.mcp_servers.push(server);
        self
    }

    /// Append a tool entry.
    #[must_use]
    pub fn tool(mut self, tool: AgentTool) -> Self {
        self.tools.push(tool);
        self
    }

    /// Finalize.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidConfig`](crate::Error::InvalidConfig)
    /// if `name` or `model` was not set.
    pub fn build(self) -> Result<CreateAgentRequest> {
        let name = self
            .name
            .ok_or_else(|| crate::Error::InvalidConfig("name is required".into()))?;
        let model = self
            .model
            .ok_or_else(|| crate::Error::InvalidConfig("model is required".into()))?;
        Ok(CreateAgentRequest {
            name,
            model,
            system: self.system,
            mcp_servers: self.mcp_servers,
            tools: self.tools,
        })
    }
}

/// An agent definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Agent {
    /// Stable identifier (`agent_...`).
    pub id: String,
    /// Wire type tag (`"agent"`).
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub ty: Option<String>,
    /// Agent name.
    pub name: String,
    /// Latest version number on this agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<u32>,
    /// Model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelId>,
    /// Optional system prompt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    /// Configured MCP servers.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mcp_servers: Vec<AgentMcpServer>,
    /// Configured tools.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<AgentTool>,
    /// Creation timestamp.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
}

// =====================================================================
// Namespace handle
// =====================================================================

/// Namespace handle for the Agents API. Currently exposes `create`
/// only; full CRUD lands in a follow-up release.
pub struct Agents<'a> {
    client: &'a Client,
}

impl<'a> Agents<'a> {
    pub(crate) fn new(client: &'a Client) -> Self {
        Self { client }
    }

    /// `POST /v1/agents`.
    pub async fn create(&self, request: CreateAgentRequest) -> Result<Agent> {
        let body = &request;
        self.client
            .execute_with_retry(
                || {
                    self.client
                        .request_builder(reqwest::Method::POST, "/v1/agents")
                        .json(body)
                },
                &[MANAGED_AGENTS_BETA],
            )
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn client_for(mock: &MockServer) -> Client {
        Client::builder()
            .api_key("sk-ant-test")
            .base_url(mock.uri())
            .build()
            .unwrap()
    }

    #[test]
    fn agent_tool_serializes_agent_toolset_with_type_tag() {
        let v = serde_json::to_value(AgentTool::agent_toolset()).unwrap();
        assert_eq!(v, json!({"type": "agent_toolset_20260401"}));
    }

    #[test]
    fn agent_tool_serializes_mcp_toolset_with_server_name() {
        let v = serde_json::to_value(AgentTool::mcp_toolset("github")).unwrap();
        assert_eq!(
            v,
            json!({"type": "mcp_toolset", "mcp_server_name": "github"})
        );
    }

    #[test]
    fn agent_tool_passthrough_other_variant() {
        let raw = json!({"type": "future_tool", "x": 1});
        let parsed: AgentTool = serde_json::from_value(raw.clone()).unwrap();
        match parsed {
            AgentTool::Other(v) => assert_eq!(v, raw),
            AgentTool::AgentToolset20260401 { .. } | AgentTool::McpToolset { .. } => {
                panic!("expected Other")
            }
        }
    }

    #[tokio::test]
    async fn create_agent_posts_full_payload() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/agents"))
            .and(body_partial_json(json!({
                "name": "Code Reviewer",
                "model": "claude-opus-4-7",
                "system": "Reviewer.",
                "mcp_servers": [
                    {"type": "url", "name": "github", "url": "https://api.githubcopilot.com/mcp/"}
                ],
                "tools": [
                    {"type": "agent_toolset_20260401"},
                    {"type": "mcp_toolset", "mcp_server_name": "github"}
                ]
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "agent_01",
                "type": "agent",
                "name": "Code Reviewer",
                "version": 1
            })))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let req = CreateAgentRequest::builder()
            .name("Code Reviewer")
            .model(ModelId::OPUS_4_7)
            .system("Reviewer.")
            .mcp_server(AgentMcpServer::url(
                "github",
                "https://api.githubcopilot.com/mcp/",
            ))
            .tool(AgentTool::agent_toolset())
            .tool(AgentTool::mcp_toolset("github"))
            .build()
            .unwrap();
        let agent = client.managed_agents().agents().create(req).await.unwrap();
        assert_eq!(agent.id, "agent_01");
        assert_eq!(agent.version, Some(1));
    }
}
