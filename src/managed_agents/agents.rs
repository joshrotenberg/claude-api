//! Agents: full CRUD + version history.
//!
//! An agent defines how Claude behaves within a session: model
//! (optionally with `fast` inference speed), system prompt, MCP
//! servers, skills, tools (built-in toolset + MCP toolsets + custom
//! tools), description, metadata. Agents are versioned -- creating one
//! starts at version 1 and any [`Agents::update`] increments the
//! version. Sessions reference agents either at the latest version
//! (string ID) or pinned to a specific version (see
//! [`AgentRef`](super::sessions::AgentRef)).
//!
//! # Forward compatibility
//!
//! All wrapper enums in this module follow the
//! `Known | Other(serde_json::Value)` pattern. Brand-new server
//! variants -- a new permission policy, a new toolset shape, a new
//! skill type -- deserialize into `Other` preserving the raw JSON, so
//! upgrading the server without upgrading the SDK doesn't break parsing.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::client::Client;
use crate::error::Result;
use crate::pagination::Paginated;

use super::MANAGED_AGENTS_BETA;

// =====================================================================
// Model + speed
// =====================================================================

/// Inference speed mode. `Fast` charges premium pricing; not all models
/// support it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ModelSpeed {
    /// Default inference speed.
    Standard,
    /// Premium-priced fast inference.
    Fast,
}

/// Model identifier. Wire form is either a bare string or a
/// `{id, speed}` object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum AgentModel {
    /// Bare model string. Use this for the common case where you don't
    /// need to override the inference speed.
    String(String),
    /// Object form with explicit speed.
    Config {
        /// Model ID (e.g. `claude-opus-4-7`).
        id: String,
        /// Inference speed.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        speed: Option<ModelSpeed>,
    },
}

impl AgentModel {
    /// Bare model string (latest speed default).
    #[must_use]
    pub fn id(id: impl Into<String>) -> Self {
        Self::String(id.into())
    }

    /// Model with an explicit speed override.
    #[must_use]
    pub fn config(id: impl Into<String>, speed: ModelSpeed) -> Self {
        Self::Config {
            id: id.into(),
            speed: Some(speed),
        }
    }

    /// Borrow the underlying model ID regardless of variant.
    #[must_use]
    pub fn model_id(&self) -> &str {
        match self {
            Self::String(s) => s,
            Self::Config { id, .. } => id,
        }
    }
}

impl From<&str> for AgentModel {
    fn from(s: &str) -> Self {
        Self::String(s.to_owned())
    }
}

impl From<String> for AgentModel {
    fn from(s: String) -> Self {
        Self::String(s)
    }
}

impl From<crate::types::ModelId> for AgentModel {
    fn from(m: crate::types::ModelId) -> Self {
        Self::String(m.as_str().to_owned())
    }
}

// =====================================================================
// MCP server
// =====================================================================

/// MCP server reference on an agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum AgentMcpServer {
    /// HTTP(S)-reachable MCP server.
    Url {
        /// Server name; referenced by `mcp_toolset` configs.
        name: String,
        /// Endpoint URL.
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

// =====================================================================
// Permission policies
// =====================================================================

/// Permission policy controlling whether tool calls require user
/// confirmation. Forward-compatible: unknown policy types fall through
/// to [`Self::Other`].
#[derive(Debug, Clone, PartialEq)]
pub enum PermissionPolicy {
    /// Auto-approve every invocation.
    AlwaysAllow,
    /// Require a `user.tool_confirmation` event before each invocation.
    AlwaysAsk,
    /// Unknown policy type; raw JSON preserved.
    Other(serde_json::Value),
}

const KNOWN_PERMISSION_POLICY_TAGS: &[&str] = &["always_allow", "always_ask"];

impl Serialize for PermissionPolicy {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        match self {
            Self::AlwaysAllow => {
                let mut map = s.serialize_map(Some(1))?;
                map.serialize_entry("type", "always_allow")?;
                map.end()
            }
            Self::AlwaysAsk => {
                let mut map = s.serialize_map(Some(1))?;
                map.serialize_entry("type", "always_ask")?;
                map.end()
            }
            Self::Other(v) => v.serialize(s),
        }
    }
}

impl<'de> Deserialize<'de> for PermissionPolicy {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let raw = serde_json::Value::deserialize(d)?;
        let tag = raw.get("type").and_then(serde_json::Value::as_str);
        match tag {
            Some("always_allow") if KNOWN_PERMISSION_POLICY_TAGS.contains(&"always_allow") => {
                Ok(Self::AlwaysAllow)
            }
            Some("always_ask") => Ok(Self::AlwaysAsk),
            _ => Ok(Self::Other(raw)),
        }
    }
}

// =====================================================================
// Toolset configs
// =====================================================================

/// Built-in agent tool identifier.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum BuiltinToolName {
    /// Shell.
    #[default]
    Bash,
    /// File edit.
    Edit,
    /// File read.
    Read,
    /// File write.
    Write,
    /// Glob match.
    Glob,
    /// Grep search.
    Grep,
    /// Fetch a URL.
    WebFetch,
    /// Web search.
    WebSearch,
}

/// Per-tool override on a built-in toolset.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub struct BuiltinToolConfig {
    /// Tool identifier.
    pub name: BuiltinToolName,
    /// Whether this tool is enabled. Overrides the toolset default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// Permission policy. Overrides the toolset default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_policy: Option<PermissionPolicy>,
}

/// Per-tool override on an MCP toolset.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub struct McpToolConfig {
    /// MCP tool name.
    pub name: String,
    /// Whether this tool is enabled. Overrides the toolset default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// Permission policy. Overrides the toolset default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_policy: Option<PermissionPolicy>,
}

/// Default configuration for all tools in a toolset.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ToolsetDefaultConfig {
    /// Whether tools are enabled by default. Defaults to `true`
    /// server-side when omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// Default permission policy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_policy: Option<PermissionPolicy>,
}

/// JSON Schema for a custom tool's input parameters.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CustomToolInputSchema {
    /// JSON Schema `properties` map.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub properties: Option<serde_json::Value>,
    /// Required property names.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required: Vec<String>,
    /// Always `"object"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "type")]
    pub ty: Option<String>,
}

/// Custom tool executed by the API client (not the agent). Calls
/// surface as `agent.custom_tool_use` events; respond with
/// `user.custom_tool_result`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CustomTool {
    /// Tool name. 1-128 chars; letters, digits, underscores, hyphens.
    pub name: String,
    /// Description shown to the agent. 1-1024 chars.
    pub description: String,
    /// Input JSON Schema.
    pub input_schema: CustomToolInputSchema,
}

// =====================================================================
// AgentTool wrapper
// =====================================================================

/// One tool entry on an agent.
///
/// Forward-compatible: unknown wire `type` tags fall through to
/// [`Self::Other`] preserving the raw JSON.
#[derive(Debug, Clone, PartialEq)]
pub enum AgentTool {
    /// Pre-built `agent_toolset_20260401` (bash / edit / read / write /
    /// glob / grep / `web_fetch` / `web_search`) with optional per-tool
    /// overrides and a default config.
    BuiltinToolset(BuiltinToolset),
    /// MCP toolset bound to a server name from the agent's
    /// `mcp_servers` array.
    McpToolset(McpToolset),
    /// Custom client-executed tool.
    Custom(CustomTool),
    /// Unknown tool kind; raw JSON preserved.
    Other(serde_json::Value),
}

/// Body of [`AgentTool::BuiltinToolset`].
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub struct BuiltinToolset {
    /// Per-tool overrides.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub configs: Vec<BuiltinToolConfig>,
    /// Default config applied to tools not overridden in `configs`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_config: Option<ToolsetDefaultConfig>,
}

/// Body of [`AgentTool::McpToolset`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct McpToolset {
    /// MCP server name from the agent's `mcp_servers` array.
    pub mcp_server_name: String,
    /// Per-tool overrides.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub configs: Vec<McpToolConfig>,
    /// Default config applied to tools not overridden in `configs`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_config: Option<ToolsetDefaultConfig>,
}

const KNOWN_AGENT_TOOL_TAGS: &[&str] = &["agent_toolset_20260401", "mcp_toolset", "custom"];

impl Serialize for AgentTool {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        match self {
            Self::BuiltinToolset(b) => {
                let mut map = s.serialize_map(None)?;
                map.serialize_entry("type", "agent_toolset_20260401")?;
                if !b.configs.is_empty() {
                    map.serialize_entry("configs", &b.configs)?;
                }
                if let Some(d) = &b.default_config {
                    map.serialize_entry("default_config", d)?;
                }
                map.end()
            }
            Self::McpToolset(m) => {
                let mut map = s.serialize_map(None)?;
                map.serialize_entry("type", "mcp_toolset")?;
                map.serialize_entry("mcp_server_name", &m.mcp_server_name)?;
                if !m.configs.is_empty() {
                    map.serialize_entry("configs", &m.configs)?;
                }
                if let Some(d) = &m.default_config {
                    map.serialize_entry("default_config", d)?;
                }
                map.end()
            }
            Self::Custom(c) => {
                let mut map = s.serialize_map(None)?;
                map.serialize_entry("type", "custom")?;
                map.serialize_entry("name", &c.name)?;
                map.serialize_entry("description", &c.description)?;
                map.serialize_entry("input_schema", &c.input_schema)?;
                map.end()
            }
            Self::Other(v) => v.serialize(s),
        }
    }
}

impl<'de> Deserialize<'de> for AgentTool {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let raw = serde_json::Value::deserialize(d)?;
        let tag = raw.get("type").and_then(serde_json::Value::as_str);
        match tag {
            Some("agent_toolset_20260401")
                if KNOWN_AGENT_TOOL_TAGS.contains(&"agent_toolset_20260401") =>
            {
                let b = serde_json::from_value::<BuiltinToolset>(raw)
                    .map_err(serde::de::Error::custom)?;
                Ok(Self::BuiltinToolset(b))
            }
            Some("mcp_toolset") => {
                let m =
                    serde_json::from_value::<McpToolset>(raw).map_err(serde::de::Error::custom)?;
                Ok(Self::McpToolset(m))
            }
            Some("custom") => {
                let c =
                    serde_json::from_value::<CustomTool>(raw).map_err(serde::de::Error::custom)?;
                Ok(Self::Custom(c))
            }
            _ => Ok(Self::Other(raw)),
        }
    }
}

impl AgentTool {
    /// Enable the pre-built `agent_toolset_20260401` (no overrides).
    #[must_use]
    pub fn builtin_toolset() -> Self {
        Self::BuiltinToolset(BuiltinToolset::default())
    }

    /// Expose tools from a named MCP server.
    #[must_use]
    pub fn mcp_toolset(server_name: impl Into<String>) -> Self {
        Self::McpToolset(McpToolset {
            mcp_server_name: server_name.into(),
            configs: Vec::new(),
            default_config: None,
        })
    }

    /// Build a custom tool.
    #[must_use]
    pub fn custom(
        name: impl Into<String>,
        description: impl Into<String>,
        input_schema: CustomToolInputSchema,
    ) -> Self {
        Self::Custom(CustomTool {
            name: name.into(),
            description: description.into(),
            input_schema,
        })
    }
}

// =====================================================================
// Skills
// =====================================================================

/// A skill referenced on an agent. Forward-compatible: unknown skill
/// types fall through to [`Self::Other`].
#[derive(Debug, Clone, PartialEq)]
pub enum Skill {
    /// Anthropic-managed skill.
    Anthropic(AnthropicSkill),
    /// User-created custom skill.
    Custom(CustomSkill),
    /// Unknown skill type; raw JSON preserved.
    Other(serde_json::Value),
}

/// Body of [`Skill::Anthropic`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct AnthropicSkill {
    /// Skill ID (e.g. `"xlsx"`).
    pub skill_id: String,
    /// Pinned version. Defaults to latest if omitted on requests; the
    /// resolved version is always echoed on responses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// Body of [`Skill::Custom`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CustomSkill {
    /// Skill ID (e.g. `"skill_01XJ5..."`).
    pub skill_id: String,
    /// Pinned version. Defaults to latest if omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

const KNOWN_SKILL_TAGS: &[&str] = &["anthropic", "custom"];

impl Serialize for Skill {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        match self {
            Self::Anthropic(a) => {
                let mut map = s.serialize_map(None)?;
                map.serialize_entry("type", "anthropic")?;
                map.serialize_entry("skill_id", &a.skill_id)?;
                if let Some(v) = &a.version {
                    map.serialize_entry("version", v)?;
                }
                map.end()
            }
            Self::Custom(c) => {
                let mut map = s.serialize_map(None)?;
                map.serialize_entry("type", "custom")?;
                map.serialize_entry("skill_id", &c.skill_id)?;
                if let Some(v) = &c.version {
                    map.serialize_entry("version", v)?;
                }
                map.end()
            }
            Self::Other(v) => v.serialize(s),
        }
    }
}

impl<'de> Deserialize<'de> for Skill {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let raw = serde_json::Value::deserialize(d)?;
        let tag = raw.get("type").and_then(serde_json::Value::as_str);
        match tag {
            Some("anthropic") if KNOWN_SKILL_TAGS.contains(&"anthropic") => {
                let a = serde_json::from_value::<AnthropicSkill>(raw)
                    .map_err(serde::de::Error::custom)?;
                Ok(Self::Anthropic(a))
            }
            Some("custom") => {
                let c =
                    serde_json::from_value::<CustomSkill>(raw).map_err(serde::de::Error::custom)?;
                Ok(Self::Custom(c))
            }
            _ => Ok(Self::Other(raw)),
        }
    }
}

impl Skill {
    /// Reference an Anthropic-managed skill (latest version).
    #[must_use]
    pub fn anthropic(skill_id: impl Into<String>) -> Self {
        Self::Anthropic(AnthropicSkill {
            skill_id: skill_id.into(),
            version: None,
        })
    }

    /// Reference an Anthropic-managed skill pinned to a version.
    #[must_use]
    pub fn anthropic_pinned(skill_id: impl Into<String>, version: impl Into<String>) -> Self {
        Self::Anthropic(AnthropicSkill {
            skill_id: skill_id.into(),
            version: Some(version.into()),
        })
    }

    /// Reference a user-created custom skill (latest version).
    #[must_use]
    pub fn custom(skill_id: impl Into<String>) -> Self {
        Self::Custom(CustomSkill {
            skill_id: skill_id.into(),
            version: None,
        })
    }
}

// =====================================================================
// Callable agents (multi-agent / threads)
// =====================================================================

/// Reference to another agent that this agent is permitted to call.
///
/// Used in [`CreateAgentRequest::callable_agents`] and
/// [`UpdateAgentRequest::callable_agents`] when configuring a
/// multi-agent coordinator. At runtime, when the coordinator delegates
/// to one of these, the platform spawns a new
/// [`Thread`](crate::managed_agents::threads::Thread).
///
/// Wire form is `{type: "agent", id, version}` -- the same shape as
/// the pinned [`AgentRef`](super::sessions::AgentRef).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CallableAgent {
    /// Always `"agent"`.
    #[serde(rename = "type")]
    pub ty: String,
    /// Agent ID.
    pub id: String,
    /// Pinned version. Required.
    pub version: u32,
}

impl CallableAgent {
    /// Build a callable-agent reference pinned to a version.
    #[must_use]
    pub fn new(id: impl Into<String>, version: u32) -> Self {
        Self {
            ty: "agent".into(),
            id: id.into(),
            version,
        }
    }
}

// =====================================================================
// Agent (response shape)
// =====================================================================

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
    /// Description. May be `null` on the wire when no description was
    /// provided at create time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Model identifier and configuration.
    pub model: AgentModel,
    /// System prompt. May be `null` on the wire when no system prompt
    /// was provided at create time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    /// Configured MCP servers.
    #[serde(default)]
    pub mcp_servers: Vec<AgentMcpServer>,
    /// Configured skills.
    #[serde(default)]
    pub skills: Vec<Skill>,
    /// Configured tools.
    #[serde(default)]
    pub tools: Vec<AgentTool>,
    /// Free-form metadata.
    #[serde(default)]
    pub metadata: HashMap<String, String>,
    /// Other agents this agent is permitted to call (multi-agent
    /// coordinators only). Empty for non-coordinator agents.
    #[serde(default)]
    pub callable_agents: Vec<CallableAgent>,
    /// Current version. Starts at 1, increments on every update.
    pub version: u32,
    /// Creation timestamp (RFC3339).
    pub created_at: String,
    /// Last-modified timestamp (RFC3339).
    pub updated_at: String,
    /// Set when archived (RFC3339), `None` for live agents.
    #[serde(default)]
    pub archived_at: Option<String>,
}

// =====================================================================
// Create request
// =====================================================================

/// Request body for `POST /v1/agents`.
#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct CreateAgentRequest {
    /// Agent name. 1-256 chars.
    pub name: String,
    /// Model.
    pub model: AgentModel,
    /// Description. Up to 2048 chars.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// System prompt. Up to 100,000 chars.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    /// MCP servers. Max 20.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub mcp_servers: Vec<AgentMcpServer>,
    /// Skills. Max 20.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<Skill>,
    /// Tools. Max 128 across all toolsets.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<AgentTool>,
    /// Metadata. Max 16 pairs (64-char keys, 512-char values).
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, String>,
    /// Other agents this agent is permitted to call. Setting this
    /// makes the agent a multi-agent coordinator: at runtime,
    /// delegations spawn new
    /// [`Thread`](crate::managed_agents::threads::Thread)s. Only one
    /// level of delegation is supported -- callable agents cannot
    /// themselves have callable agents.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub callable_agents: Vec<CallableAgent>,
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
    model: Option<AgentModel>,
    description: Option<String>,
    system: Option<String>,
    mcp_servers: Vec<AgentMcpServer>,
    skills: Vec<Skill>,
    tools: Vec<AgentTool>,
    metadata: HashMap<String, String>,
    callable_agents: Vec<CallableAgent>,
}

impl CreateAgentRequestBuilder {
    /// Set the name. Required.
    #[must_use]
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Set the model. Required.
    #[must_use]
    pub fn model(mut self, model: impl Into<AgentModel>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Set the description.
    #[must_use]
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Set the system prompt.
    #[must_use]
    pub fn system(mut self, system: impl Into<String>) -> Self {
        self.system = Some(system.into());
        self
    }

    /// Append an MCP server.
    #[must_use]
    pub fn mcp_server(mut self, server: AgentMcpServer) -> Self {
        self.mcp_servers.push(server);
        self
    }

    /// Append a skill.
    #[must_use]
    pub fn skill(mut self, skill: Skill) -> Self {
        self.skills.push(skill);
        self
    }

    /// Append a tool.
    #[must_use]
    pub fn tool(mut self, tool: AgentTool) -> Self {
        self.tools.push(tool);
        self
    }

    /// Insert a metadata entry.
    #[must_use]
    pub fn metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Append a callable-agent reference (for multi-agent coordinators).
    #[must_use]
    pub fn callable_agent(mut self, callable: CallableAgent) -> Self {
        self.callable_agents.push(callable);
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
            description: self.description,
            system: self.system,
            mcp_servers: self.mcp_servers,
            skills: self.skills,
            tools: self.tools,
            metadata: self.metadata,
            callable_agents: self.callable_agents,
        })
    }
}

// =====================================================================
// Update request
// =====================================================================

/// Request body for `POST /v1/agents/{id}` (update).
///
/// **Optimistic concurrency**: pass the agent's current `version`. The
/// request is rejected if the server's stored version no longer
/// matches.
///
/// **Field semantics**:
/// - `Option::None` → omit the field → preserve the existing value.
/// - `Option::Some` with empty string / empty array / `null` → clear,
///   except for `name` and `model` which cannot be cleared.
/// - For `metadata`, see [`MetadataPatch`] for per-key delete semantics.
#[derive(Debug, Clone, Default, Serialize)]
#[non_exhaustive]
pub struct UpdateAgentRequest {
    /// Current version, used for optimistic concurrency. Required.
    pub version: u32,
    /// New name (cannot be cleared).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// New model (cannot be cleared).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<AgentModel>,
    /// New description. `Some("")` clears.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// New system prompt. `Some("")` clears.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    /// Replacement MCP-servers list. `Some(vec![])` clears.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcp_servers: Option<Vec<AgentMcpServer>>,
    /// Replacement skills list. `Some(vec![])` clears.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skills: Option<Vec<Skill>>,
    /// Replacement tools list. `Some(vec![])` clears.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<AgentTool>>,
    /// Per-key metadata patch. See [`MetadataPatch`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<MetadataPatch>,
    /// Replacement callable-agents list. `Some(vec![])` clears (turns
    /// the agent back into a non-coordinator).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub callable_agents: Option<Vec<CallableAgent>>,
}

impl UpdateAgentRequest {
    /// Build a minimal update request (pass `version`, then chain
    /// setters).
    #[must_use]
    pub fn at_version(version: u32) -> Self {
        Self {
            version,
            ..Self::default()
        }
    }

    /// Set the name.
    #[must_use]
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Set the model.
    #[must_use]
    pub fn model(mut self, model: impl Into<AgentModel>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Set or clear (`Some("")`) the description.
    #[must_use]
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Set or clear (`Some("")`) the system prompt.
    #[must_use]
    pub fn system(mut self, system: impl Into<String>) -> Self {
        self.system = Some(system.into());
        self
    }

    /// Replace the MCP servers list. Pass an empty Vec to clear.
    #[must_use]
    pub fn mcp_servers(mut self, servers: Vec<AgentMcpServer>) -> Self {
        self.mcp_servers = Some(servers);
        self
    }

    /// Replace the skills list. Pass an empty Vec to clear.
    #[must_use]
    pub fn skills(mut self, skills: Vec<Skill>) -> Self {
        self.skills = Some(skills);
        self
    }

    /// Replace the tools list. Pass an empty Vec to clear.
    #[must_use]
    pub fn tools(mut self, tools: Vec<AgentTool>) -> Self {
        self.tools = Some(tools);
        self
    }

    /// Apply a metadata patch.
    #[must_use]
    pub fn metadata(mut self, patch: MetadataPatch) -> Self {
        self.metadata = Some(patch);
        self
    }

    /// Replace the callable-agents list. Pass an empty Vec to clear.
    #[must_use]
    pub fn callable_agents(mut self, callable: Vec<CallableAgent>) -> Self {
        self.callable_agents = Some(callable);
        self
    }
}

/// Metadata patch on [`UpdateAgentRequest`]. Each entry is either a
/// `String` (upsert that key to the given value) or `None` (delete that
/// key). Keys not present in the patch are preserved.
#[derive(Debug, Clone, Default, Serialize)]
pub struct MetadataPatch(pub HashMap<String, Option<String>>);

impl MetadataPatch {
    /// Empty patch.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Upsert a key to a value.
    #[must_use]
    pub fn set(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.0.insert(key.into(), Some(value.into()));
        self
    }

    /// Delete a key.
    #[must_use]
    pub fn delete(mut self, key: impl Into<String>) -> Self {
        self.0.insert(key.into(), None);
        self
    }
}

// =====================================================================
// List params
// =====================================================================

/// Optional knobs for [`Agents::list`].
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct ListAgentsParams {
    /// Created at or after this RFC3339 time.
    pub created_at_gte: Option<String>,
    /// Created at or before this RFC3339 time.
    pub created_at_lte: Option<String>,
    /// Include archived agents. Defaults to `false`.
    pub include_archived: Option<bool>,
    /// Page size. Default 20, max 100.
    pub limit: Option<u32>,
    /// Pagination cursor from a previous response's `next_page`.
    pub page: Option<String>,
}

impl ListAgentsParams {
    fn to_query(&self) -> Vec<(&'static str, String)> {
        let mut q = Vec::new();
        if let Some(t) = &self.created_at_gte {
            q.push(("created_at[gte]", t.clone()));
        }
        if let Some(t) = &self.created_at_lte {
            q.push(("created_at[lte]", t.clone()));
        }
        if let Some(b) = self.include_archived {
            q.push(("include_archived", b.to_string()));
        }
        if let Some(l) = self.limit {
            q.push(("limit", l.to_string()));
        }
        if let Some(p) = &self.page {
            q.push(("page", p.clone()));
        }
        q
    }
}

/// Optional knobs for [`Agents::list_versions`].
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct ListAgentVersionsParams {
    /// Page size. Default 20, max 100.
    pub limit: Option<u32>,
    /// Pagination cursor.
    pub page: Option<String>,
}

impl ListAgentVersionsParams {
    fn to_query(&self) -> Vec<(&'static str, String)> {
        let mut q = Vec::new();
        if let Some(l) = self.limit {
            q.push(("limit", l.to_string()));
        }
        if let Some(p) = &self.page {
            q.push(("page", p.clone()));
        }
        q
    }
}

// =====================================================================
// Namespace handle
// =====================================================================

/// Namespace handle for the Agents API.
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

    /// `GET /v1/agents/{id}`. Pass `version = Some(n)` to retrieve a
    /// specific historical version; `None` returns the latest.
    pub async fn retrieve(&self, agent_id: &str, version: Option<u32>) -> Result<Agent> {
        let path = format!("/v1/agents/{agent_id}");
        let v = version;
        self.client
            .execute_with_retry(
                || {
                    let mut req = self.client.request_builder(reqwest::Method::GET, &path);
                    if let Some(version) = v {
                        req = req.query(&[("version", version.to_string())]);
                    }
                    req
                },
                &[MANAGED_AGENTS_BETA],
            )
            .await
    }

    /// `GET /v1/agents`.
    pub async fn list(&self, params: ListAgentsParams) -> Result<Paginated<Agent>> {
        let query = params.to_query();
        self.client
            .execute_with_retry(
                || {
                    let mut req = self
                        .client
                        .request_builder(reqwest::Method::GET, "/v1/agents");
                    for (k, v) in &query {
                        req = req.query(&[(k, v)]);
                    }
                    req
                },
                &[MANAGED_AGENTS_BETA],
            )
            .await
    }

    /// `POST /v1/agents/{id}` (update). Bumps the version on success.
    /// Fails with HTTP 409 if `request.version` doesn't match the
    /// server's current version.
    pub async fn update(&self, agent_id: &str, request: UpdateAgentRequest) -> Result<Agent> {
        let path = format!("/v1/agents/{agent_id}");
        let body = &request;
        self.client
            .execute_with_retry(
                || {
                    self.client
                        .request_builder(reqwest::Method::POST, &path)
                        .json(body)
                },
                &[MANAGED_AGENTS_BETA],
            )
            .await
    }

    /// `POST /v1/agents/{id}/archive`.
    pub async fn archive(&self, agent_id: &str) -> Result<Agent> {
        let path = format!("/v1/agents/{agent_id}/archive");
        self.client
            .execute_with_retry(
                || self.client.request_builder(reqwest::Method::POST, &path),
                &[MANAGED_AGENTS_BETA],
            )
            .await
    }

    /// `GET /v1/agents/{id}/versions`. Returns the agent's full version
    /// history, newest first.
    pub async fn list_versions(
        &self,
        agent_id: &str,
        params: ListAgentVersionsParams,
    ) -> Result<Paginated<Agent>> {
        let path = format!("/v1/agents/{agent_id}/versions");
        let query = params.to_query();
        self.client
            .execute_with_retry(
                || {
                    let mut req = self.client.request_builder(reqwest::Method::GET, &path);
                    for (k, v) in &query {
                        req = req.query(&[(k, v)]);
                    }
                    req
                },
                &[MANAGED_AGENTS_BETA],
            )
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
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

    fn fake_agent_response() -> serde_json::Value {
        json!({
            "id": "agent_01",
            "type": "agent",
            "name": "Reviewer",
            "description": "",
            "model": "claude-opus-4-7",
            "system": "",
            "mcp_servers": [],
            "skills": [],
            "tools": [],
            "metadata": {},
            "version": 1,
            "created_at": "2026-04-30T12:00:00Z",
            "updated_at": "2026-04-30T12:00:00Z"
        })
    }

    #[test]
    fn agent_model_serializes_string_form_untagged() {
        let m = AgentModel::id("claude-opus-4-7");
        let v = serde_json::to_value(&m).unwrap();
        assert_eq!(v, json!("claude-opus-4-7"));
    }

    #[test]
    fn agent_model_serializes_config_form_with_speed() {
        let m = AgentModel::config("claude-opus-4-7", ModelSpeed::Fast);
        let v = serde_json::to_value(&m).unwrap();
        assert_eq!(v, json!({"id": "claude-opus-4-7", "speed": "fast"}));
        let parsed: AgentModel = serde_json::from_value(v).unwrap();
        assert_eq!(parsed, m);
    }

    #[test]
    fn permission_policy_round_trips_known_variants() {
        assert_eq!(
            serde_json::to_value(PermissionPolicy::AlwaysAllow).unwrap(),
            json!({"type": "always_allow"})
        );
        assert_eq!(
            serde_json::to_value(PermissionPolicy::AlwaysAsk).unwrap(),
            json!({"type": "always_ask"})
        );
    }

    #[test]
    fn permission_policy_unknown_variant_falls_to_other() {
        let raw = json!({"type": "future_policy", "x": 1});
        let parsed: PermissionPolicy = serde_json::from_value(raw.clone()).unwrap();
        match parsed {
            PermissionPolicy::Other(v) => assert_eq!(v, raw),
            PermissionPolicy::AlwaysAllow | PermissionPolicy::AlwaysAsk => panic!("expected Other"),
        }
    }

    #[test]
    fn agent_tool_builtin_toolset_serializes_with_configs() {
        let tool = AgentTool::BuiltinToolset(BuiltinToolset {
            configs: vec![BuiltinToolConfig {
                name: BuiltinToolName::Bash,
                enabled: Some(true),
                permission_policy: Some(PermissionPolicy::AlwaysAsk),
            }],
            default_config: Some(ToolsetDefaultConfig {
                enabled: Some(true),
                permission_policy: Some(PermissionPolicy::AlwaysAllow),
            }),
        });
        let v = serde_json::to_value(&tool).unwrap();
        assert_eq!(v["type"], "agent_toolset_20260401");
        assert_eq!(v["configs"][0]["name"], "bash");
        assert_eq!(v["configs"][0]["permission_policy"]["type"], "always_ask");
        assert_eq!(v["default_config"]["enabled"], true);
    }

    #[test]
    fn agent_tool_mcp_toolset_round_trips_with_server_name() {
        let tool = AgentTool::mcp_toolset("github");
        let v = serde_json::to_value(&tool).unwrap();
        assert_eq!(
            v,
            json!({"type": "mcp_toolset", "mcp_server_name": "github"})
        );
        let parsed: AgentTool = serde_json::from_value(v).unwrap();
        assert_eq!(parsed, tool);
    }

    #[test]
    fn agent_tool_custom_round_trips_with_input_schema() {
        let tool = AgentTool::custom(
            "lookup",
            "Find a record by id",
            CustomToolInputSchema {
                properties: Some(json!({"id": {"type": "string"}})),
                required: vec!["id".into()],
                ty: Some("object".into()),
            },
        );
        let v = serde_json::to_value(&tool).unwrap();
        assert_eq!(v["type"], "custom");
        assert_eq!(v["name"], "lookup");
        assert_eq!(v["input_schema"]["required"], json!(["id"]));
        let parsed: AgentTool = serde_json::from_value(v).unwrap();
        assert_eq!(parsed, tool);
    }

    #[test]
    fn agent_tool_unknown_kind_falls_to_other() {
        let raw = json!({"type": "future_tool", "x": 1});
        let parsed: AgentTool = serde_json::from_value(raw.clone()).unwrap();
        match parsed {
            AgentTool::Other(v) => assert_eq!(v, raw),
            AgentTool::BuiltinToolset(_) | AgentTool::McpToolset(_) | AgentTool::Custom(_) => {
                panic!("expected Other")
            }
        }
    }

    #[test]
    fn skill_round_trips_anthropic_and_custom_with_version() {
        let a = Skill::anthropic_pinned("xlsx", "1.2.3");
        let v = serde_json::to_value(&a).unwrap();
        assert_eq!(
            v,
            json!({"type": "anthropic", "skill_id": "xlsx", "version": "1.2.3"})
        );
        let parsed: Skill = serde_json::from_value(v).unwrap();
        assert_eq!(parsed, a);

        let c = Skill::custom("skill_01XJ5");
        let v = serde_json::to_value(&c).unwrap();
        assert_eq!(v, json!({"type": "custom", "skill_id": "skill_01XJ5"}));
    }

    #[test]
    fn skill_unknown_type_falls_to_other() {
        let raw = json!({"type": "future_skill", "blob": 1});
        let parsed: Skill = serde_json::from_value(raw.clone()).unwrap();
        match parsed {
            Skill::Other(v) => assert_eq!(v, raw),
            Skill::Anthropic(_) | Skill::Custom(_) => panic!("expected Other"),
        }
    }

    #[test]
    fn metadata_patch_serializes_set_and_delete() {
        let p = MetadataPatch::new().set("env", "prod").delete("legacy_key");
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(v["env"], "prod");
        assert_eq!(v["legacy_key"], serde_json::Value::Null);
    }

    #[tokio::test]
    async fn create_agent_posts_minimal_payload() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/agents"))
            .and(body_partial_json(json!({
                "name": "Reviewer",
                "model": "claude-opus-4-7"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(fake_agent_response()))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let req = CreateAgentRequest::builder()
            .name("Reviewer")
            .model("claude-opus-4-7")
            .build()
            .unwrap();
        let agent = client.managed_agents().agents().create(req).await.unwrap();
        assert_eq!(agent.id, "agent_01");
        assert_eq!(agent.version, 1);
    }

    #[tokio::test]
    async fn create_coordinator_agent_includes_callable_agents() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/agents"))
            .and(body_partial_json(json!({
                "name": "Engineering Lead",
                "model": "claude-opus-4-7",
                "callable_agents": [
                    {"type": "agent", "id": "agent_reviewer", "version": 2},
                    {"type": "agent", "id": "agent_test_writer", "version": 5}
                ]
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json({
                let mut r = fake_agent_response();
                r["callable_agents"] = json!([
                    {"type": "agent", "id": "agent_reviewer", "version": 2},
                    {"type": "agent", "id": "agent_test_writer", "version": 5}
                ]);
                r
            }))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let req = CreateAgentRequest::builder()
            .name("Engineering Lead")
            .model("claude-opus-4-7")
            .callable_agent(CallableAgent::new("agent_reviewer", 2))
            .callable_agent(CallableAgent::new("agent_test_writer", 5))
            .build()
            .unwrap();
        let agent = client.managed_agents().agents().create(req).await.unwrap();
        assert_eq!(agent.callable_agents.len(), 2);
        assert_eq!(agent.callable_agents[0].id, "agent_reviewer");
        assert_eq!(agent.callable_agents[0].version, 2);
    }

    #[test]
    fn callable_agent_serializes_with_type_tag() {
        let c = CallableAgent::new("agent_x", 3);
        let v = serde_json::to_value(&c).unwrap();
        assert_eq!(v, json!({"type": "agent", "id": "agent_x", "version": 3}));
    }

    #[tokio::test]
    async fn create_agent_full_payload_round_trips() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/agents"))
            .and(body_partial_json(json!({
                "name": "Reviewer",
                "model": {"id": "claude-opus-4-7", "speed": "fast"},
                "system": "Be helpful.",
                "description": "Code review assistant.",
                "mcp_servers": [
                    {"type": "url", "name": "github", "url": "https://api.githubcopilot.com/mcp/"}
                ],
                "tools": [
                    {"type": "agent_toolset_20260401"},
                    {"type": "mcp_toolset", "mcp_server_name": "github"}
                ],
                "skills": [{"type": "anthropic", "skill_id": "xlsx"}],
                "metadata": {"env": "prod"}
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(fake_agent_response()))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let req = CreateAgentRequest::builder()
            .name("Reviewer")
            .model(AgentModel::config("claude-opus-4-7", ModelSpeed::Fast))
            .system("Be helpful.")
            .description("Code review assistant.")
            .mcp_server(AgentMcpServer::url(
                "github",
                "https://api.githubcopilot.com/mcp/",
            ))
            .tool(AgentTool::builtin_toolset())
            .tool(AgentTool::mcp_toolset("github"))
            .skill(Skill::anthropic("xlsx"))
            .metadata("env", "prod")
            .build()
            .unwrap();
        client.managed_agents().agents().create(req).await.unwrap();
    }

    #[tokio::test]
    async fn retrieve_agent_passes_version_query_when_supplied() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/agents/agent_01"))
            .and(wiremock::matchers::query_param("version", "3"))
            .respond_with(ResponseTemplate::new(200).set_body_json(fake_agent_response()))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let _ = client
            .managed_agents()
            .agents()
            .retrieve("agent_01", Some(3))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn list_agents_passes_created_at_brackets_in_query() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/agents"))
            .and(wiremock::matchers::query_param(
                "created_at[gte]",
                "2026-04-01T00:00:00Z",
            ))
            .and(wiremock::matchers::query_param("include_archived", "true"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [fake_agent_response()],
                "next_page": null
            })))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let page = client
            .managed_agents()
            .agents()
            .list(ListAgentsParams {
                created_at_gte: Some("2026-04-01T00:00:00Z".into()),
                include_archived: Some(true),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(page.data.len(), 1);
    }

    #[tokio::test]
    async fn update_agent_sends_version_for_optimistic_concurrency() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/agents/agent_01"))
            .and(body_partial_json(json!({
                "version": 1,
                "name": "Reviewer v2",
                "metadata": {"env": "staging", "old_key": null}
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(fake_agent_response()))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let req = UpdateAgentRequest::at_version(1)
            .name("Reviewer v2")
            .metadata(MetadataPatch::new().set("env", "staging").delete("old_key"));
        client
            .managed_agents()
            .agents()
            .update("agent_01", req)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn archive_agent_posts_to_archive_subpath() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/agents/agent_01/archive"))
            .respond_with(ResponseTemplate::new(200).set_body_json({
                let mut a = fake_agent_response();
                a["archived_at"] = json!("2026-04-30T12:00:00Z");
                a
            }))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let agent = client
            .managed_agents()
            .agents()
            .archive("agent_01")
            .await
            .unwrap();
        assert!(agent.archived_at.is_some());
    }

    #[tokio::test]
    async fn list_versions_returns_paginated_history() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/agents/agent_01/versions"))
            .and(wiremock::matchers::query_param("limit", "5"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [fake_agent_response()],
                "next_page": "cursor_x"
            })))
            .mount(&mock)
            .await;

        let client = client_for(&mock);
        let page = client
            .managed_agents()
            .agents()
            .list_versions(
                "agent_01",
                ListAgentVersionsParams {
                    limit: Some(5),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(page.data.len(), 1);
    }
}
