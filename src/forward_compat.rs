//! Shared helper for forward-compatible tagged enums.
//!
//! Several wire-format types in this crate (e.g. [`crate::messages::ContentBlock`]
//! and the streaming events in [`crate::messages::stream`]) follow the same
//! pattern: an outer enum that wraps either a known variant (typed) or an
//! unrecognized payload (raw JSON, preserved verbatim).
//!
//! Strict-on-known semantics: a known `type` tag whose body fails to
//! deserialize returns an error rather than silently falling through to the
//! `Other` arm. Genuine validation bugs surface; only true forward-compat
//! cases land in `Other`.
//!
//! This module is crate-private.

use serde::de::DeserializeOwned;

/// Deserialize a JSON [`serde_json::Value`] into either a known variant `K`
/// or wrap it as the unknown-payload arm.
///
/// `known_tags` is the set of `type` strings the caller's `K` enum recognizes.
/// `wrap_known` and `wrap_other` build the outer enum from each branch.
pub(crate) fn dispatch_known_or_other<K, T>(
    raw: serde_json::Value,
    known_tags: &[&str],
    wrap_known: impl FnOnce(K) -> T,
    wrap_other: impl FnOnce(serde_json::Value) -> T,
) -> Result<T, serde_json::Error>
where
    K: DeserializeOwned,
{
    let type_tag = raw.get("type").and_then(serde_json::Value::as_str);
    match type_tag {
        Some(t) if known_tags.contains(&t) => {
            let known: K = serde_json::from_value(raw)?;
            Ok(wrap_known(known))
        }
        _ => Ok(wrap_other(raw)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde::{Deserialize, Serialize};
    use serde_json::json;

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    #[serde(tag = "type", rename_all = "snake_case")]
    enum KnownTestEvent {
        Foo { x: u32 },
        Bar { y: String },
    }

    #[derive(Debug, Clone, PartialEq)]
    enum TestEvent {
        Known(KnownTestEvent),
        Other(serde_json::Value),
    }

    const TAGS: &[&str] = &["foo", "bar"];

    fn parse(raw: serde_json::Value) -> Result<TestEvent, serde_json::Error> {
        dispatch_known_or_other(raw, TAGS, TestEvent::Known, TestEvent::Other)
    }

    #[test]
    fn known_tag_decodes_to_known() {
        let ev = parse(json!({"type": "foo", "x": 7})).unwrap();
        assert_eq!(ev, TestEvent::Known(KnownTestEvent::Foo { x: 7 }));
    }

    #[test]
    fn unknown_tag_falls_to_other() {
        let raw = json!({"type": "future", "data": [1, 2, 3]});
        let ev = parse(raw.clone()).unwrap();
        assert_eq!(ev, TestEvent::Other(raw));
    }

    #[test]
    fn missing_tag_falls_to_other() {
        let raw = json!({"x": 1});
        let ev = parse(raw.clone()).unwrap();
        assert_eq!(ev, TestEvent::Other(raw));
    }

    #[test]
    fn malformed_known_tag_errors() {
        // Tag matches "foo" but `x` is wrong type.
        let raw = json!({"type": "foo", "x": "not a number"});
        assert!(parse(raw).is_err(), "must not silently fall to Other");
    }
}
