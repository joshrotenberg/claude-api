//! MCP server configuration for the `mcp_servers` request field.
//!
//! Pass one or more [`McpServerConfig`] values when constructing a
//! [`CreateMessageRequest`](crate::messages::request::CreateMessageRequest)
//! to give the model access to Model Context Protocol tools hosted at
//! external URLs. The server must speak the MCP protocol over HTTP/SSE.

use serde::{Deserialize, Serialize};

/// One entry in the `mcp_servers` array on a Messages request.
///
/// Currently only the URL form is supported on the wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum McpServerConfig {
    /// A URL-addressable MCP server.
    Url {
        /// MCP endpoint URL.
        url: String,
        /// Logical name for this server (used in tool dispatch).
        name: String,
        /// Optional bearer token for the MCP server.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        authorization_token: Option<String>,
        /// Per-server tool gating.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tool_configuration: Option<McpToolConfiguration>,
    },
}

impl McpServerConfig {
    /// Convenience constructor for a URL-addressed MCP server.
    pub fn url(url: impl Into<String>, name: impl Into<String>) -> Self {
        Self::Url {
            url: url.into(),
            name: name.into(),
            authorization_token: None,
            tool_configuration: None,
        }
    }
}

/// Per-server tool gating for an [`McpServerConfig::Url`].
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub struct McpToolConfiguration {
    /// Whether the model is allowed to use tools from this MCP server.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// If set, restrict the model to this allowlist of tool names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_tools: Option<Vec<String>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn url_minimal_round_trips() {
        let c = McpServerConfig::url("https://mcp.example", "example");
        let v = serde_json::to_value(&c).unwrap();
        assert_eq!(
            v,
            json!({"type": "url", "url": "https://mcp.example", "name": "example"})
        );
        let parsed: McpServerConfig = serde_json::from_value(v).unwrap();
        assert_eq!(parsed, c);
    }

    #[test]
    fn url_full_round_trips() {
        let c = McpServerConfig::Url {
            url: "https://mcp.example".into(),
            name: "example".into(),
            authorization_token: Some("Bearer xyz".into()),
            tool_configuration: Some(McpToolConfiguration {
                enabled: Some(true),
                allowed_tools: Some(vec!["search".into(), "fetch".into()]),
            }),
        };
        let v = serde_json::to_value(&c).unwrap();
        assert_eq!(
            v,
            json!({
                "type": "url",
                "url": "https://mcp.example",
                "name": "example",
                "authorization_token": "Bearer xyz",
                "tool_configuration": {
                    "enabled": true,
                    "allowed_tools": ["search", "fetch"]
                }
            })
        );
        let parsed: McpServerConfig = serde_json::from_value(v).unwrap();
        assert_eq!(parsed, c);
    }
}
