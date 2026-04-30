//! Generic pagination wrapper used across paginated endpoints.
//!
//! Anthropic's list endpoints return:
//!
//! ```json
//! {
//!     "data": [...],
//!     "has_more": false,
//!     "first_id": "...",
//!     "last_id": "..."
//! }
//! ```
//!
//! [`Paginated<T>`] models that envelope. Caller-driven paging via
//! [`Paginated::next_after`] / [`Paginated::next_before`]; auto-paginating
//! collectors (e.g. `Models::list_all`) live on each endpoint and return
//! `Vec<T>` for v0.1.

use serde::{Deserialize, Serialize};

/// One page of items returned from a paginated list endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Paginated<T> {
    /// Items on this page.
    pub data: Vec<T>,
    /// Whether more pages exist after this one.
    #[serde(default)]
    pub has_more: bool,
    /// ID of the first item on this page (cursor for `before_id`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_id: Option<String>,
    /// ID of the last item on this page (cursor for `after_id`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_id: Option<String>,
}

impl<T> Paginated<T> {
    /// Cursor for the next page (forward direction): the `last_id` of this
    /// page, suitable for the next request's `after_id` parameter. Returns
    /// `None` if there are no more pages.
    pub fn next_after(&self) -> Option<&str> {
        if self.has_more {
            self.last_id.as_deref()
        } else {
            None
        }
    }

    /// Cursor for the previous page (backward direction): the `first_id`
    /// of this page, for the next request's `before_id` parameter.
    pub fn next_before(&self) -> Option<&str> {
        self.first_id.as_deref()
    }

    /// Whether the page itself is empty.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn paginated_round_trips_full_envelope() {
        let p = Paginated {
            data: vec!["a".to_owned(), "b".to_owned()],
            has_more: true,
            first_id: Some("first".into()),
            last_id: Some("last".into()),
        };
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(
            v,
            json!({
                "data": ["a", "b"],
                "has_more": true,
                "first_id": "first",
                "last_id": "last"
            })
        );
        let parsed: Paginated<String> = serde_json::from_value(v).unwrap();
        assert_eq!(parsed, p);
    }

    #[test]
    fn paginated_tolerates_missing_optional_fields() {
        let raw = json!({"data": [1, 2, 3]});
        let p: Paginated<i32> = serde_json::from_value(raw).unwrap();
        assert_eq!(p.data, vec![1, 2, 3]);
        assert!(!p.has_more);
        assert_eq!(p.first_id, None);
        assert_eq!(p.last_id, None);
    }

    #[test]
    fn next_after_returns_last_id_only_when_more_pages() {
        let p = Paginated::<i32> {
            data: vec![1],
            has_more: true,
            first_id: Some("f".into()),
            last_id: Some("l".into()),
        };
        assert_eq!(p.next_after(), Some("l"));

        let p_done = Paginated::<i32> {
            has_more: false,
            ..p
        };
        assert_eq!(p_done.next_after(), None);
    }
}
