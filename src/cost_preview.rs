//! `CostPreview` -- estimate the USD cost of a request before sending it.
//!
//! The input side is exact: it hits `/v1/messages/count_tokens` to get the
//! tokenizer's actual count. The output side is bounded: we use
//! `request.max_tokens` as the upper bound, since the actual number of
//! output tokens is unknown until generation finishes. Use
//! [`CostPreview::cost_for`] for a point estimate at any specific output
//! count.
//!
//! Obtain via [`Messages::cost_preview`](crate::messages::Messages::cost_preview).

#![cfg(all(feature = "async", feature = "pricing"))]

use crate::pricing::PricingTable;
use crate::types::{ModelId, Usage};

/// Pre-flight cost estimate for a request.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct CostPreview {
    /// Model the estimate was computed for.
    pub model: ModelId,
    /// Server-counted input tokens (from `/v1/messages/count_tokens`).
    pub input_tokens: u32,
    /// Output upper bound, taken from `request.max_tokens`.
    pub max_output_tokens: u32,
    /// USD cost of the input tokens alone.
    pub input_cost_usd: f64,
    /// USD cost if the model emits exactly `max_output_tokens` output tokens.
    pub max_output_cost_usd: f64,
    /// `input_cost_usd + max_output_cost_usd`. The largest amount this
    /// request could cost in vanilla usage (excludes cache/server-tool
    /// charges since those are runtime-determined).
    pub max_total_usd: f64,
}

impl CostPreview {
    /// USD cost if the model produces exactly `output_tokens` tokens. Useful
    /// for plotting expected cost against an empirical output-size estimate.
    #[must_use]
    pub fn cost_for(&self, output_tokens: u32, pricing: &PricingTable) -> f64 {
        pricing.cost(
            &self.model,
            &Usage {
                input_tokens: self.input_tokens,
                output_tokens,
                ..Usage::default()
            },
        )
    }
}
