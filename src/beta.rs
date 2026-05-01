//! Typed `anthropic-beta` header values.
//!
//! [`BetaHeader`] enumerates the canonical beta version strings
//! published in Anthropic's API reference. It's an open-string enum:
//! known values map to typed variants for autocompletion and
//! refactor safety, and unknown values fall through to
//! [`BetaHeader::Other`] so a brand-new beta header can still be
//! passed by string without a crate update.
//!
//! Pass [`BetaHeader`] values directly to
//! [`ClientBuilder::beta`](crate::ClientBuilder::beta) -- the
//! `impl Into<String>` bound is satisfied via [`From<BetaHeader> for
//! String`].
//!
//! ```ignore
//! use claude_api::{Client, BetaHeader};
//!
//! let client = Client::builder()
//!     .api_key("sk-ant-...")
//!     .beta(BetaHeader::Skills)
//!     .beta(BetaHeader::UserProfiles)
//!     .build()?;
//! # Ok::<(), claude_api::Error>(())
//! ```

use serde::{Deserialize, Serialize};

/// Canonical Anthropic beta header values.
///
/// This list is derived from the public API reference and may grow
/// over time. New values added by the API will deserialize into
/// [`Self::Other`] until this enum is updated.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BetaHeader {
    /// `message-batches-2024-09-24`
    MessageBatches,
    /// `prompt-caching-2024-07-31`
    PromptCaching,
    /// `computer-use-2024-10-22`
    ComputerUse20241022,
    /// `computer-use-2025-01-24`
    ComputerUse20250124,
    /// `pdfs-2024-09-25`
    Pdfs,
    /// `token-counting-2024-11-01`
    TokenCounting,
    /// `token-efficient-tools-2025-02-19`
    TokenEfficientTools,
    /// `output-128k-2025-02-19`
    Output128k,
    /// `files-api-2025-04-14`
    FilesApi,
    /// `mcp-client-2025-04-04`
    McpClient20250404,
    /// `mcp-client-2025-11-20`
    McpClient20251120,
    /// `dev-full-thinking-2025-05-14`
    DevFullThinking,
    /// `interleaved-thinking-2025-05-14`
    InterleavedThinking,
    /// `code-execution-2025-05-22`
    CodeExecution,
    /// `extended-cache-ttl-2025-04-11`
    ExtendedCacheTtl,
    /// `context-1m-2025-08-07`
    Context1m,
    /// `context-management-2025-06-27`
    ContextManagement,
    /// `model-context-window-exceeded-2025-08-26`
    ModelContextWindowExceeded,
    /// `skills-2025-10-02`
    Skills,
    /// `fast-mode-2026-02-01`
    FastMode,
    /// `output-300k-2026-03-24`
    Output300k,
    /// `user-profiles-2026-03-24`
    UserProfiles,
    /// `advisor-tool-2026-03-01`
    AdvisorTool,
    /// Forward-compat fallback for beta headers added by Anthropic
    /// after this enum was last updated. Round-trips byte-for-byte.
    Other(String),
}

impl BetaHeader {
    /// Wire-format string sent in the `anthropic-beta` HTTP header.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::MessageBatches => "message-batches-2024-09-24",
            Self::PromptCaching => "prompt-caching-2024-07-31",
            Self::ComputerUse20241022 => "computer-use-2024-10-22",
            Self::ComputerUse20250124 => "computer-use-2025-01-24",
            Self::Pdfs => "pdfs-2024-09-25",
            Self::TokenCounting => "token-counting-2024-11-01",
            Self::TokenEfficientTools => "token-efficient-tools-2025-02-19",
            Self::Output128k => "output-128k-2025-02-19",
            Self::FilesApi => "files-api-2025-04-14",
            Self::McpClient20250404 => "mcp-client-2025-04-04",
            Self::McpClient20251120 => "mcp-client-2025-11-20",
            Self::DevFullThinking => "dev-full-thinking-2025-05-14",
            Self::InterleavedThinking => "interleaved-thinking-2025-05-14",
            Self::CodeExecution => "code-execution-2025-05-22",
            Self::ExtendedCacheTtl => "extended-cache-ttl-2025-04-11",
            Self::Context1m => "context-1m-2025-08-07",
            Self::ContextManagement => "context-management-2025-06-27",
            Self::ModelContextWindowExceeded => "model-context-window-exceeded-2025-08-26",
            Self::Skills => "skills-2025-10-02",
            Self::FastMode => "fast-mode-2026-02-01",
            Self::Output300k => "output-300k-2026-03-24",
            Self::UserProfiles => "user-profiles-2026-03-24",
            Self::AdvisorTool => "advisor-tool-2026-03-01",
            Self::Other(v) => v,
        }
    }

    /// Whether this is a recognized known variant (not [`Self::Other`]).
    #[must_use]
    pub fn is_known(&self) -> bool {
        !matches!(self, Self::Other(_))
    }
}

impl std::fmt::Display for BetaHeader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<BetaHeader> for String {
    fn from(b: BetaHeader) -> Self {
        b.as_str().to_owned()
    }
}

impl From<&BetaHeader> for String {
    fn from(b: &BetaHeader) -> Self {
        b.as_str().to_owned()
    }
}

impl From<String> for BetaHeader {
    fn from(s: String) -> Self {
        Self::from_wire(&s).unwrap_or(Self::Other(s))
    }
}

impl From<&str> for BetaHeader {
    fn from(s: &str) -> Self {
        Self::from_wire(s).unwrap_or_else(|| Self::Other(s.to_owned()))
    }
}

impl BetaHeader {
    /// Parse a wire-format string into a known variant. Returns `None`
    /// for unknown values; callers wrap into [`Self::Other`].
    fn from_wire(s: &str) -> Option<Self> {
        Some(match s {
            "message-batches-2024-09-24" => Self::MessageBatches,
            "prompt-caching-2024-07-31" => Self::PromptCaching,
            "computer-use-2024-10-22" => Self::ComputerUse20241022,
            "computer-use-2025-01-24" => Self::ComputerUse20250124,
            "pdfs-2024-09-25" => Self::Pdfs,
            "token-counting-2024-11-01" => Self::TokenCounting,
            "token-efficient-tools-2025-02-19" => Self::TokenEfficientTools,
            "output-128k-2025-02-19" => Self::Output128k,
            "files-api-2025-04-14" => Self::FilesApi,
            "mcp-client-2025-04-04" => Self::McpClient20250404,
            "mcp-client-2025-11-20" => Self::McpClient20251120,
            "dev-full-thinking-2025-05-14" => Self::DevFullThinking,
            "interleaved-thinking-2025-05-14" => Self::InterleavedThinking,
            "code-execution-2025-05-22" => Self::CodeExecution,
            "extended-cache-ttl-2025-04-11" => Self::ExtendedCacheTtl,
            "context-1m-2025-08-07" => Self::Context1m,
            "context-management-2025-06-27" => Self::ContextManagement,
            "model-context-window-exceeded-2025-08-26" => Self::ModelContextWindowExceeded,
            "skills-2025-10-02" => Self::Skills,
            "fast-mode-2026-02-01" => Self::FastMode,
            "output-300k-2026-03-24" => Self::Output300k,
            "user-profiles-2026-03-24" => Self::UserProfiles,
            "advisor-tool-2026-03-01" => Self::AdvisorTool,
            _ => return None,
        })
    }
}

impl Serialize for BetaHeader {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for BetaHeader {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(Self::from(s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    /// All 23 canonical wire strings, in the order they appear on the
    /// API reference. Pin against drift.
    const ALL_WIRE: &[&str] = &[
        "message-batches-2024-09-24",
        "prompt-caching-2024-07-31",
        "computer-use-2024-10-22",
        "computer-use-2025-01-24",
        "pdfs-2024-09-25",
        "token-counting-2024-11-01",
        "token-efficient-tools-2025-02-19",
        "output-128k-2025-02-19",
        "files-api-2025-04-14",
        "mcp-client-2025-04-04",
        "mcp-client-2025-11-20",
        "dev-full-thinking-2025-05-14",
        "interleaved-thinking-2025-05-14",
        "code-execution-2025-05-22",
        "extended-cache-ttl-2025-04-11",
        "context-1m-2025-08-07",
        "context-management-2025-06-27",
        "model-context-window-exceeded-2025-08-26",
        "skills-2025-10-02",
        "fast-mode-2026-02-01",
        "output-300k-2026-03-24",
        "user-profiles-2026-03-24",
        "advisor-tool-2026-03-01",
    ];

    #[test]
    fn every_known_wire_value_round_trips_through_from_and_as_str() {
        for &wire in ALL_WIRE {
            let parsed = BetaHeader::from(wire);
            assert!(
                parsed.is_known(),
                "expected {wire} to parse to a known variant, got {parsed:?}"
            );
            assert_eq!(parsed.as_str(), wire, "round-trip mismatch on {wire}");
        }
    }

    #[test]
    fn unknown_values_fall_through_to_other_and_round_trip() {
        let unknown = BetaHeader::from("brand-new-beta-2099-12-31");
        assert!(!unknown.is_known());
        assert_eq!(unknown.as_str(), "brand-new-beta-2099-12-31");
        let s: String = unknown.clone().into();
        assert_eq!(s, "brand-new-beta-2099-12-31");
    }

    #[test]
    fn serde_round_trips_known_and_unknown_values() {
        let known = BetaHeader::Skills;
        let json = serde_json::to_string(&known).unwrap();
        assert_eq!(json, r#""skills-2025-10-02""#);
        let back: BetaHeader = serde_json::from_str(&json).unwrap();
        assert_eq!(back, BetaHeader::Skills);

        let unknown = BetaHeader::from("custom-x");
        let json = serde_json::to_string(&unknown).unwrap();
        assert_eq!(json, r#""custom-x""#);
        let back: BetaHeader = serde_json::from_str(&json).unwrap();
        assert_eq!(back, BetaHeader::Other("custom-x".into()));
    }

    #[test]
    fn display_matches_wire_format() {
        assert_eq!(format!("{}", BetaHeader::Skills), "skills-2025-10-02");
        assert_eq!(
            format!("{}", BetaHeader::UserProfiles),
            "user-profiles-2026-03-24"
        );
    }

    #[test]
    fn into_string_works_for_client_builder_beta() {
        // Simulates what `client.beta(BetaHeader::Skills)` does internally
        // via `impl Into<String>`.
        let v: String = BetaHeader::Skills.into();
        assert_eq!(v, "skills-2025-10-02");
        let v: String = (&BetaHeader::UserProfiles).into();
        assert_eq!(v, "user-profiles-2026-03-24");
    }
}
