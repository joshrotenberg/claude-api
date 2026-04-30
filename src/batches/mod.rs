//! The Batches API: submit a batch of message requests, poll for
//! completion, stream per-request results.
//!
//! Anthropic's batch endpoint is the cheapest way to run large fan-out
//! workloads (50% off vs. per-request pricing) at the cost of higher
//! latency. This module wraps the full surface:
//!
//! - [`Batches::create`] -- submit
//! - [`Batches::get`] -- status (polling-friendly)
//! - [`Batches::list`] / [`Batches::list_all`] -- enumerate
//! - [`Batches::cancel`], [`Batches::delete`]
//! - [`Batches::wait_for`] -- poller that returns once `ended_at` is set
//! - [`Batches::results`] / [`Batches::results_stream`] -- decode the JSONL
//!   results body, eagerly into a `Vec` or lazily as a `Stream`
//!
//! The batch ID is the only state you need to durably persist; reattach
//! later by calling [`Batches::get(id)`](Batches::get) or
//! [`Batches::wait_for(id, _)`](Batches::wait_for).

pub mod types;

#[cfg(feature = "async")]
#[cfg_attr(docsrs, doc(cfg(feature = "async")))]
pub mod api;

pub use types::{
    BatchDeleted, BatchRequest, BatchResultItem, BatchResultPayload, ListBatchesParams,
    MessageBatch, ProcessingStatus, RequestCounts, WaitOptions,
};

#[cfg(feature = "async")]
pub use api::Batches;
