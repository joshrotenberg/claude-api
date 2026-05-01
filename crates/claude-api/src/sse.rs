//! Low-level Server-Sent Events parsing.
//!
//! Wraps the `eventsource-stream` crate and maps each SSE event's `data` payload
//! into a typed value via [`serde_json`]. The high-level
//! [`EventStream`](crate::messages::stream::EventStream) sits on top of this.
//!
//! Gated on the `streaming` feature.

use eventsource_stream::Eventsource;
use futures_util::StreamExt;

use crate::error::{Error, Result, StreamError};

/// Convert a streaming HTTP response body into a stream of typed values.
///
/// Each SSE event's `data` field is JSON-parsed into `T`. Wire-level parse
/// errors map to [`StreamError::Connection`]; JSON-decode errors map to
/// [`StreamError::Parse`].
pub(crate) fn into_typed_stream<T>(
    response: reqwest::Response,
) -> impl futures_util::Stream<Item = Result<T>> + Send + 'static
where
    T: serde::de::DeserializeOwned + Send + 'static,
{
    response
        .bytes_stream()
        .eventsource()
        .map(|item| match item {
            Ok(event) => serde_json::from_str::<T>(&event.data)
                .map_err(|e| Error::Stream(StreamError::Parse(e.to_string()))),
            Err(e) => Err(Error::Stream(StreamError::Connection(e.to_string()))),
        })
}
