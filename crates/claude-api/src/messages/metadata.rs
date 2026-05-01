//! Per-request metadata and the request-side `service_tier` field.
//!
//! Note: the request-side [`RequestServiceTier`] (`auto` / `standard_only`)
//! is distinct from the response-side
//! [`ServiceTier`](crate::types::ServiceTier) (`standard` / `priority` / `batch`).

use serde::{Deserialize, Serialize};

/// Optional metadata sent with a Messages request.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub struct MessageMetadata {
    /// Stable user identifier; passed through to abuse-detection systems.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
}

impl MessageMetadata {
    /// Convenience constructor that sets `user_id`.
    pub fn with_user(user_id: impl Into<String>) -> Self {
        Self {
            user_id: Some(user_id.into()),
        }
    }
}

/// Request-side `service_tier` field on a Messages request.
///
/// Differs from the response-side [`ServiceTier`](crate::types::ServiceTier),
/// which reports the tier the request *actually ran on*.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum RequestServiceTier {
    /// Server picks the best available tier.
    Auto,
    /// Restrict to standard tier only (no priority / no batch).
    StandardOnly,
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn message_metadata_round_trips() {
        let m = MessageMetadata::with_user("user_42");
        let v = serde_json::to_value(&m).unwrap();
        assert_eq!(v, json!({"user_id": "user_42"}));
        let parsed: MessageMetadata = serde_json::from_value(v).unwrap();
        assert_eq!(parsed, m);
    }

    #[test]
    fn message_metadata_default_omits_user_id() {
        let m = MessageMetadata::default();
        let v = serde_json::to_value(&m).unwrap();
        assert_eq!(v, json!({}));
    }

    #[test]
    fn request_service_tier_round_trips() {
        for (variant, wire) in [
            (RequestServiceTier::Auto, "auto"),
            (RequestServiceTier::StandardOnly, "standard_only"),
        ] {
            let v = serde_json::to_value(variant).unwrap();
            assert_eq!(v, json!(wire));
            let parsed: RequestServiceTier = serde_json::from_value(v).unwrap();
            assert_eq!(parsed, variant);
        }
    }
}
