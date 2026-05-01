//! Typed citations produced by Claude.
//!
//! [`Citation`] is the public, forward-compatible enum: it wraps a
//! [`KnownCitation`] for any citation type the SDK understands, or a raw
//! [`serde_json::Value`] for any type it doesn't. Same wrapper-enum +
//! strict-on-known pattern as [`crate::messages::content::ContentBlock`]
//! and [`crate::messages::stream::StreamEvent`].
//!
//! # Variants
//!
//! - **`CharLocation`** -- character range in a text document.
//! - **`PageLocation`** -- page range in a PDF document.
//! - **`ContentBlockLocation`** -- block range in a structured document.
//! - **`WebSearchResultLocation`** -- citation produced by the server-side
//!   web search tool.
//! - **`Other(Value)`** -- any future variant the SDK doesn't know about,
//!   preserved byte-for-byte for round-trip.

use serde::{Deserialize, Serialize};

use crate::forward_compat::dispatch_known_or_other;

/// A citation tying a span of generated text back to a source document or
/// web result.
///
/// Forward-compatible: unknown `type` tags deserialize into [`Citation::Other`]
/// with the raw JSON preserved.
#[derive(Debug, Clone, PartialEq)]
pub enum Citation {
    /// A citation whose `type` is recognized by this SDK version.
    Known(KnownCitation),
    /// A citation whose `type` is not recognized; the raw JSON is preserved.
    Other(serde_json::Value),
}

/// All citation variants known to this SDK version.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum KnownCitation {
    /// Citation tied to a character range in a text-source document.
    CharLocation {
        /// Index of the document in the request's content array.
        document_index: u32,
        /// Title of the document, if one was provided.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        document_title: Option<String>,
        /// The exact text span the model is citing.
        cited_text: String,
        /// Inclusive start character offset.
        start_char_index: u32,
        /// Exclusive end character offset.
        end_char_index: u32,
    },
    /// Citation tied to a page range in a PDF.
    PageLocation {
        /// Index of the document in the request's content array.
        document_index: u32,
        /// Title of the document, if one was provided.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        document_title: Option<String>,
        /// The exact text span the model is citing.
        cited_text: String,
        /// Inclusive start page number (1-indexed).
        start_page_number: u32,
        /// Exclusive end page number.
        end_page_number: u32,
    },
    /// Citation tied to a content-block range in a structured document.
    ContentBlockLocation {
        /// Index of the document in the request's content array.
        document_index: u32,
        /// Title of the document, if one was provided.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        document_title: Option<String>,
        /// The exact text span the model is citing.
        cited_text: String,
        /// Inclusive start block index.
        start_block_index: u32,
        /// Exclusive end block index.
        end_block_index: u32,
    },
    /// Citation produced by the server-side `web_search` built-in tool.
    WebSearchResultLocation {
        /// Source URL.
        url: String,
        /// Page title.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        /// The exact text span the model is citing.
        cited_text: String,
        /// Opaque encrypted index used by the server to resolve the search hit.
        encrypted_index: String,
    },
}

const KNOWN_CITATION_TAGS: &[&str] = &[
    "char_location",
    "page_location",
    "content_block_location",
    "web_search_result_location",
];

impl Serialize for Citation {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Citation::Known(k) => k.serialize(s),
            Citation::Other(v) => v.serialize(s),
        }
    }
}

impl<'de> Deserialize<'de> for Citation {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = serde_json::Value::deserialize(d)?;
        dispatch_known_or_other(raw, KNOWN_CITATION_TAGS, Citation::Known, Citation::Other)
            .map_err(serde::de::Error::custom)
    }
}

impl From<KnownCitation> for Citation {
    fn from(k: KnownCitation) -> Self {
        Citation::Known(k)
    }
}

impl Citation {
    /// If this is a known citation, return the inner [`KnownCitation`].
    #[must_use]
    pub fn known(&self) -> Option<&KnownCitation> {
        match self {
            Self::Known(k) => Some(k),
            Self::Other(_) => None,
        }
    }

    /// If this is an unknown citation, return the raw JSON.
    #[must_use]
    pub fn other(&self) -> Option<&serde_json::Value> {
        match self {
            Self::Other(v) => Some(v),
            Self::Known(_) => None,
        }
    }

    /// Wire-level `type` tag for this citation regardless of variant.
    #[must_use]
    pub fn type_tag(&self) -> Option<&str> {
        match self {
            Self::Known(k) => Some(known_citation_tag(k)),
            Self::Other(v) => v.get("type").and_then(serde_json::Value::as_str),
        }
    }

    /// The text span the model cited. Available on every known variant
    /// and best-effort for [`Citation::Other`].
    #[must_use]
    pub fn cited_text(&self) -> Option<&str> {
        match self {
            Self::Known(k) => Some(match k {
                KnownCitation::CharLocation { cited_text, .. }
                | KnownCitation::PageLocation { cited_text, .. }
                | KnownCitation::ContentBlockLocation { cited_text, .. }
                | KnownCitation::WebSearchResultLocation { cited_text, .. } => cited_text,
            }),
            Self::Other(v) => v.get("cited_text").and_then(serde_json::Value::as_str),
        }
    }

    /// Title of the source (document title or web page title), when available.
    #[must_use]
    pub fn title(&self) -> Option<&str> {
        match self {
            Self::Known(
                KnownCitation::CharLocation { document_title, .. }
                | KnownCitation::PageLocation { document_title, .. }
                | KnownCitation::ContentBlockLocation { document_title, .. },
            ) => document_title.as_deref(),
            Self::Known(KnownCitation::WebSearchResultLocation { title, .. }) => title.as_deref(),
            Self::Other(v) => v
                .get("document_title")
                .or_else(|| v.get("title"))
                .and_then(serde_json::Value::as_str),
        }
    }
}

fn known_citation_tag(k: &KnownCitation) -> &'static str {
    match k {
        KnownCitation::CharLocation { .. } => "char_location",
        KnownCitation::PageLocation { .. } => "page_location",
        KnownCitation::ContentBlockLocation { .. } => "content_block_location",
        KnownCitation::WebSearchResultLocation { .. } => "web_search_result_location",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    fn round_trip(citation: &Citation, expected: &serde_json::Value) {
        let v = serde_json::to_value(citation).expect("serialize");
        assert_eq!(&v, expected, "wire form mismatch");
        let parsed: Citation = serde_json::from_value(v).expect("deserialize");
        assert_eq!(&parsed, citation, "round-trip mismatch");
    }

    #[test]
    fn char_location_round_trips() {
        let c = Citation::Known(KnownCitation::CharLocation {
            document_index: 0,
            document_title: Some("Spec".into()),
            cited_text: "hello world".into(),
            start_char_index: 10,
            end_char_index: 21,
        });
        round_trip(
            &c,
            &json!({
                "type": "char_location",
                "document_index": 0,
                "document_title": "Spec",
                "cited_text": "hello world",
                "start_char_index": 10,
                "end_char_index": 21
            }),
        );
    }

    #[test]
    fn char_location_with_no_title_round_trips() {
        let c = Citation::Known(KnownCitation::CharLocation {
            document_index: 1,
            document_title: None,
            cited_text: "x".into(),
            start_char_index: 0,
            end_char_index: 1,
        });
        round_trip(
            &c,
            &json!({
                "type": "char_location",
                "document_index": 1,
                "cited_text": "x",
                "start_char_index": 0,
                "end_char_index": 1
            }),
        );
    }

    #[test]
    fn page_location_round_trips() {
        let c = Citation::Known(KnownCitation::PageLocation {
            document_index: 2,
            document_title: Some("Manual".into()),
            cited_text: "see page 5".into(),
            start_page_number: 5,
            end_page_number: 6,
        });
        round_trip(
            &c,
            &json!({
                "type": "page_location",
                "document_index": 2,
                "document_title": "Manual",
                "cited_text": "see page 5",
                "start_page_number": 5,
                "end_page_number": 6
            }),
        );
    }

    #[test]
    fn content_block_location_round_trips() {
        let c = Citation::Known(KnownCitation::ContentBlockLocation {
            document_index: 0,
            document_title: None,
            cited_text: "block excerpt".into(),
            start_block_index: 3,
            end_block_index: 5,
        });
        round_trip(
            &c,
            &json!({
                "type": "content_block_location",
                "document_index": 0,
                "cited_text": "block excerpt",
                "start_block_index": 3,
                "end_block_index": 5
            }),
        );
    }

    #[test]
    fn web_search_result_location_round_trips() {
        let c = Citation::Known(KnownCitation::WebSearchResultLocation {
            url: "https://example.com/post".into(),
            title: Some("Example Post".into()),
            cited_text: "the relevant snippet".into(),
            encrypted_index: "opaque-cursor-token".into(),
        });
        round_trip(
            &c,
            &json!({
                "type": "web_search_result_location",
                "url": "https://example.com/post",
                "title": "Example Post",
                "cited_text": "the relevant snippet",
                "encrypted_index": "opaque-cursor-token"
            }),
        );
    }

    #[test]
    fn unknown_citation_type_falls_back_to_other_preserving_json() {
        let raw = json!({
            "type": "future_location",
            "cited_text": "preserved",
            "extra_field": [1, 2, 3]
        });
        let c: Citation = serde_json::from_value(raw.clone()).expect("deserialize");
        match &c {
            Citation::Other(v) => assert_eq!(v, &raw),
            Citation::Known(_) => panic!("expected Other"),
        }
        let reserialized = serde_json::to_value(&c).expect("serialize");
        assert_eq!(reserialized, raw, "Other must round-trip byte-for-byte");
    }

    #[test]
    fn malformed_known_citation_is_an_error() {
        // type matches but start_char_index is wrong shape.
        let raw = json!({
            "type": "char_location",
            "document_index": 0,
            "cited_text": "x",
            "start_char_index": "nope",
            "end_char_index": 1
        });
        let result: Result<Citation, _> = serde_json::from_value(raw);
        assert!(
            result.is_err(),
            "malformed known citation must error, not silently fall through"
        );
    }

    #[test]
    fn cited_text_accessor_works_across_variants() {
        for (citation, expected) in [
            (
                Citation::Known(KnownCitation::CharLocation {
                    document_index: 0,
                    document_title: None,
                    cited_text: "char".into(),
                    start_char_index: 0,
                    end_char_index: 4,
                }),
                "char",
            ),
            (
                Citation::Known(KnownCitation::WebSearchResultLocation {
                    url: "https://x".into(),
                    title: None,
                    cited_text: "web".into(),
                    encrypted_index: "i".into(),
                }),
                "web",
            ),
        ] {
            assert_eq!(citation.cited_text(), Some(expected));
        }
    }

    #[test]
    fn cited_text_works_on_other_variant() {
        let c: Citation = serde_json::from_value(json!({
            "type": "future_xyz",
            "cited_text": "fallback works"
        }))
        .unwrap();
        assert_eq!(c.cited_text(), Some("fallback works"));
    }

    #[test]
    fn title_accessor_works_across_variants() {
        let doc = Citation::Known(KnownCitation::CharLocation {
            document_index: 0,
            document_title: Some("Doc".into()),
            cited_text: "x".into(),
            start_char_index: 0,
            end_char_index: 1,
        });
        assert_eq!(doc.title(), Some("Doc"));

        let web = Citation::Known(KnownCitation::WebSearchResultLocation {
            url: "https://x".into(),
            title: Some("Web Title".into()),
            cited_text: "x".into(),
            encrypted_index: "i".into(),
        });
        assert_eq!(web.title(), Some("Web Title"));
    }

    #[test]
    fn type_tag_works_for_known_and_other() {
        let known = Citation::Known(KnownCitation::CharLocation {
            document_index: 0,
            document_title: None,
            cited_text: "x".into(),
            start_char_index: 0,
            end_char_index: 1,
        });
        assert_eq!(known.type_tag(), Some("char_location"));

        let other: Citation = serde_json::from_value(json!({"type": "future"})).unwrap();
        assert_eq!(other.type_tag(), Some("future"));
    }
}
