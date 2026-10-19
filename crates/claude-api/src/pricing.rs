//! Per-model pricing and cost calculation.
//!
//! [`PricingTable`] is a snapshot of Anthropic's published rates as of the
//! crate release. Rates are provided in **USD per million tokens**
//! (`input_per_mtok`, `output_per_mtok`, etc.). Server-tool fees are billed
//! per-request.
//!
//! Anthropic adjusts pricing periodically; treat the bundled rates as a
//! best-effort default and pin your own via [`PricingTable::custom`] for
//! billing-critical workloads. When [`PricingTable::cost`] is called for a
//! model the table doesn't know about, the function returns `0.0` and emits
//! a one-time `tracing::warn!`; [`PricingTable::try_cost`] returns `None`
//! instead. Models priced by prompt size (Claude Haiku 5.5) carry a
//! [`LongContextTier`]. Fast mode, Batch API and Priority Tier pricing are
//! not modeled.
//!
//! Gated on the `pricing` feature.
//!
//! ```
//! use claude_api::pricing::PricingTable;
//! use claude_api::types::{ModelId, Usage};
//!
//! let pricing = PricingTable::default();
//! let mut usage = Usage::default();
//! usage.input_tokens = 1_000_000;
//! usage.output_tokens = 200_000;
//! let usd = pricing.cost(&ModelId::SONNET_4_6, &usage);
//! assert!(usd > 0.0);
//! ```

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use crate::types::{ModelId, Usage};

/// Per-model pricing snapshot.
#[derive(Debug, Clone)]
pub struct PricingTable {
    rates: HashMap<ModelId, ModelPricing>,
}

/// Rates for a single model, all in USD per million tokens unless noted.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct ModelPricing {
    /// USD per million input tokens.
    pub input_per_mtok: f64,
    /// USD per million output tokens.
    pub output_per_mtok: f64,
    /// USD per million tokens written to a 5-minute-TTL cache entry.
    pub cache_creation_5m_per_mtok: f64,
    /// USD per million tokens written to a 1-hour-TTL cache entry.
    pub cache_creation_1h_per_mtok: f64,
    /// USD per million tokens read from any cache entry.
    pub cache_read_per_mtok: f64,
    /// USD per server-side `web_search` request.
    pub web_search_per_request: f64,
    /// Higher rates that apply to the whole request when the prompt is
    /// larger than [`LongContextTier::above_prompt_tokens`]. `None` for
    /// models with a single rate regardless of prompt size.
    pub long_context: Option<LongContextTier>,
}

/// Token rates for requests whose prompt exceeds a size threshold.
///
/// The prompt size is `input_tokens + cache_creation_input_tokens +
/// cache_read_input_tokens`. Above the threshold, every token category in
/// the request is billed at these rates. Server-tool fees are unchanged.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct LongContextTier {
    /// Prompt size, in tokens, above which this tier applies.
    pub above_prompt_tokens: u32,
    /// USD per million input tokens.
    pub input_per_mtok: f64,
    /// USD per million output tokens.
    pub output_per_mtok: f64,
    /// USD per million tokens written to a 5-minute-TTL cache entry.
    pub cache_creation_5m_per_mtok: f64,
    /// USD per million tokens written to a 1-hour-TTL cache entry.
    pub cache_creation_1h_per_mtok: f64,
    /// USD per million tokens read from any cache entry.
    pub cache_read_per_mtok: f64,
}

impl LongContextTier {
    /// Build a tier from input + output rates and the standard cache
    /// multipliers (5m = 1.25x input, 1h = 2x input, read = 0.1x input).
    #[must_use]
    pub const fn from_input_output(
        above_prompt_tokens: u32,
        input_per_mtok: f64,
        output_per_mtok: f64,
    ) -> Self {
        Self {
            above_prompt_tokens,
            input_per_mtok,
            output_per_mtok,
            cache_creation_5m_per_mtok: input_per_mtok * 1.25,
            cache_creation_1h_per_mtok: input_per_mtok * 2.0,
            cache_read_per_mtok: input_per_mtok * 0.1,
        }
    }
}

impl ModelPricing {
    /// Build a pricing record from input + output rates and the standard
    /// cache multipliers (5m = 1.25x input, 1h = 2x input, read = 0.1x input).
    #[must_use]
    pub const fn from_input_output(
        input_per_mtok: f64,
        output_per_mtok: f64,
        web_search_per_request: f64,
    ) -> Self {
        Self {
            input_per_mtok,
            output_per_mtok,
            cache_creation_5m_per_mtok: input_per_mtok * 1.25,
            cache_creation_1h_per_mtok: input_per_mtok * 2.0,
            cache_read_per_mtok: input_per_mtok * 0.1,
            web_search_per_request,
            long_context: None,
        }
    }

    /// Override the cache-read rate, for models whose cache reads are not
    /// priced at the standard 0.1x of input.
    #[must_use]
    pub const fn with_cache_read(mut self, cache_read_per_mtok: f64) -> Self {
        self.cache_read_per_mtok = cache_read_per_mtok;
        self
    }

    /// Attach a prompt-size tier.
    #[must_use]
    pub const fn with_long_context(mut self, tier: LongContextTier) -> Self {
        self.long_context = Some(tier);
        self
    }

    /// The token rates that apply to a request with this usage: the
    /// long-context tier when the prompt exceeds its threshold, otherwise
    /// the base rates.
    fn token_rates(&self, usage: &Usage) -> TokenRates {
        if let Some(tier) = self.long_context
            && prompt_tokens(usage) > u64::from(tier.above_prompt_tokens)
        {
            return TokenRates {
                input: tier.input_per_mtok,
                output: tier.output_per_mtok,
                cache_5m: tier.cache_creation_5m_per_mtok,
                cache_1h: tier.cache_creation_1h_per_mtok,
                cache_read: tier.cache_read_per_mtok,
            };
        }
        TokenRates {
            input: self.input_per_mtok,
            output: self.output_per_mtok,
            cache_5m: self.cache_creation_5m_per_mtok,
            cache_1h: self.cache_creation_1h_per_mtok,
            cache_read: self.cache_read_per_mtok,
        }
    }
}

struct TokenRates {
    input: f64,
    output: f64,
    cache_5m: f64,
    cache_1h: f64,
    cache_read: f64,
}

/// Prompt size used to select a [`LongContextTier`].
fn prompt_tokens(usage: &Usage) -> u64 {
    let cache_write = match &usage.cache_creation {
        Some(b) => u64::from(b.ephemeral_5m_input_tokens) + u64::from(b.ephemeral_1h_input_tokens),
        None => u64::from(usage.cache_creation_input_tokens.unwrap_or(0)),
    };
    u64::from(usage.input_tokens)
        + cache_write
        + u64::from(usage.cache_read_input_tokens.unwrap_or(0))
}

// Pulls in `bundled_rates() -> Vec<(ModelId, ModelPricing)>` generated by
// `build.rs` from `pricing.toml` at compile time.
include!(concat!(env!("OUT_DIR"), "/pricing_data.rs"));

impl Default for PricingTable {
    fn default() -> Self {
        // Bundled rates from `pricing.toml` -- best-effort. Override via
        // PricingTable::custom or PricingTable::set for billing-critical
        // workloads.
        Self {
            rates: bundled_rates().into_iter().collect(),
        }
    }
}

impl PricingTable {
    /// Build a custom pricing table from a fully populated map.
    #[must_use]
    pub fn custom(rates: HashMap<ModelId, ModelPricing>) -> Self {
        Self { rates }
    }

    /// Override or insert a rate for a single model.
    pub fn set(&mut self, model: ModelId, rates: ModelPricing) {
        self.rates.insert(model, rates);
    }

    /// Borrow the rate row for a model, if known.
    #[must_use]
    pub fn get(&self, model: &ModelId) -> Option<&ModelPricing> {
        self.rates.get(model)
    }

    /// Total cost in USD for the given usage on the given model. Returns
    /// `0.0` when the model is unknown to the table; a `tracing::warn!` is
    /// emitted once per process per missing model. Use
    /// [`PricingTable::try_cost`] to tell an unpriced model from a free
    /// request.
    #[must_use]
    pub fn cost(&self, model: &ModelId, usage: &Usage) -> f64 {
        self.cost_breakdown(model, usage).total
    }

    /// Detailed cost breakdown. Returns an all-zero breakdown (and warns
    /// once) when the model is unknown to the table.
    #[must_use]
    pub fn cost_breakdown(&self, model: &ModelId, usage: &Usage) -> CostBreakdown {
        self.try_cost_breakdown(model, usage).unwrap_or_else(|| {
            warn_missing_once(model.as_str());
            CostBreakdown::default()
        })
    }

    /// Total cost in USD, or `None` when the model is unknown to the table.
    #[must_use]
    pub fn try_cost(&self, model: &ModelId, usage: &Usage) -> Option<f64> {
        self.try_cost_breakdown(model, usage).map(|b| b.total)
    }

    /// Detailed cost breakdown, or `None` when the model is unknown to the
    /// table.
    #[must_use]
    pub fn try_cost_breakdown(&self, model: &ModelId, usage: &Usage) -> Option<CostBreakdown> {
        let rates = self.rates.get(model)?;
        let r = rates.token_rates(usage);
        let per_mtok = |tokens: u32, rate: f64| f64::from(tokens) / 1_000_000.0 * rate;

        let input = per_mtok(usage.input_tokens, r.input);
        let output = per_mtok(usage.output_tokens, r.output);

        let cache_creation = match &usage.cache_creation {
            Some(b) => {
                per_mtok(b.ephemeral_5m_input_tokens, r.cache_5m)
                    + per_mtok(b.ephemeral_1h_input_tokens, r.cache_1h)
            }
            // No per-TTL breakdown: fall back to the legacy total field
            // and assume 5-minute TTL (the more common default).
            None => per_mtok(usage.cache_creation_input_tokens.unwrap_or(0), r.cache_5m),
        };

        let cache_read = per_mtok(usage.cache_read_input_tokens.unwrap_or(0), r.cache_read);

        let server_tool_use = usage.server_tool_use.as_ref().map_or(0.0, |s| {
            f64::from(s.web_search_requests) * rates.web_search_per_request
        });

        let total = input + output + cache_creation + cache_read + server_tool_use;
        Some(CostBreakdown {
            input,
            output,
            cache_creation,
            cache_read,
            server_tool_use,
            total,
        })
    }
}

/// Per-category breakdown of a usage cost, all in USD.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[non_exhaustive]
pub struct CostBreakdown {
    /// Cost of input tokens.
    pub input: f64,
    /// Cost of output tokens.
    pub output: f64,
    /// Cost of cache writes (5m + 1h combined).
    pub cache_creation: f64,
    /// Cost of cache reads.
    pub cache_read: f64,
    /// Cost of server-side tool invocations (e.g. `web_search`).
    pub server_tool_use: f64,
    /// Sum of the above.
    pub total: f64,
}

fn warn_missing_once(model: &str) {
    static WARNED: OnceLock<Mutex<std::collections::HashSet<String>>> = OnceLock::new();
    let warned = WARNED.get_or_init(|| Mutex::new(std::collections::HashSet::new()));
    let mut guard = warned
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if guard.insert(model.to_owned()) {
        tracing::warn!(
            model,
            "claude-api: no bundled pricing data; cost() will return 0. \
             Override via PricingTable::custom or PricingTable::set."
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{CacheCreationBreakdown, ServerToolUseUsage};

    fn approx(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-9, "expected {b} (within 1e-9), got {a}");
    }

    #[test]
    fn default_pricing_covers_every_model_constant() {
        let p = PricingTable::default();
        for model in [
            ModelId::FABLE_5_1,
            ModelId::FABLE_5,
            ModelId::OPUS_5_5,
            ModelId::OPUS_5,
            ModelId::OPUS_4_8,
            ModelId::OPUS_4_7,
            ModelId::OPUS_4_6,
            ModelId::SONNET_5_5,
            ModelId::SONNET_5,
            ModelId::SONNET_4_6,
            ModelId::HAIKU_5_5,
            ModelId::HAIKU_4_5,
            ModelId::custom("claude-haiku-4-5"),
        ] {
            assert!(p.get(&model).is_some(), "no bundled rate for {model}");
        }
    }

    fn million_in_out() -> Usage {
        Usage {
            input_tokens: 1_000_000,
            output_tokens: 1_000_000,
            ..Usage::default()
        }
    }

    #[test]
    fn bundled_input_output_rates_match_list_prices() {
        let p = PricingTable::default();
        for (model, expected) in [
            (ModelId::FABLE_5_1, 60.0),
            (ModelId::OPUS_5_5, 24.0),
            (ModelId::OPUS_5, 30.0),
            (ModelId::OPUS_4_7, 30.0),
            (ModelId::SONNET_5_5, 12.0),
            (ModelId::SONNET_4_6, 18.0),
            (ModelId::HAIKU_4_5, 6.0),
        ] {
            let got = p.try_cost(&model, &million_in_out()).unwrap();
            assert!(
                (got - expected).abs() < 1e-9,
                "{model}: expected {expected}, got {got}"
            );
        }
    }

    #[test]
    fn non_standard_cache_read_rates() {
        let p = PricingTable::default();
        let usage = Usage {
            cache_read_input_tokens: Some(1_000_000),
            ..Usage::default()
        };
        approx(p.cost(&ModelId::FABLE_5_1, &usage), 0.25);
        approx(p.cost(&ModelId::OPUS_5_5, &usage), 0.20);
        approx(p.cost(&ModelId::FABLE_5, &usage), 1.00);
    }

    #[test]
    fn haiku_5_5_bills_base_rate_up_to_the_threshold() {
        let p = PricingTable::default();
        let usage = Usage {
            input_tokens: 60_000,
            cache_read_input_tokens: Some(40_000),
            output_tokens: 1_000,
            ..Usage::default()
        };
        // Prompt is exactly 100K: base tier.
        // 60K * $0.10 + 40K * $0.01 + 1K * $0.50, per MTok.
        approx(p.cost(&ModelId::HAIKU_5_5, &usage), 0.006 + 0.0004 + 0.0005);
    }

    #[test]
    fn haiku_5_5_bills_long_context_rate_above_the_threshold() {
        let p = PricingTable::default();
        let usage = Usage {
            input_tokens: 60_000,
            cache_read_input_tokens: Some(40_001),
            output_tokens: 1_000,
            ..Usage::default()
        };
        // Prompt is 100,001 tokens: every category moves to the long tier.
        // 60K * $0.50 + 40,001 * $0.05 + 1K * $2.50, per MTok.
        let b = p.cost_breakdown(&ModelId::HAIKU_5_5, &usage);
        approx(b.input, 0.03);
        approx(b.cache_read, 40_001.0 / 1_000_000.0 * 0.05);
        approx(b.output, 0.0025);
    }

    #[test]
    fn long_context_counts_cache_writes_toward_the_prompt() {
        let p = PricingTable::default();
        let usage = Usage {
            input_tokens: 1,
            cache_creation: Some(CacheCreationBreakdown {
                ephemeral_5m_input_tokens: 100_000,
                ephemeral_1h_input_tokens: 0,
            }),
            ..Usage::default()
        };
        // 100,001 prompt tokens: 5m writes at 1.25 x $0.50.
        let b = p.cost_breakdown(&ModelId::HAIKU_5_5, &usage);
        approx(b.cache_creation, 0.1 * 0.625);
    }

    #[test]
    fn try_cost_is_none_for_unknown_models() {
        let p = PricingTable::default();
        assert_eq!(
            p.try_cost(&ModelId::custom("claude-future-foo"), &million_in_out()),
            None
        );
        assert!(p.try_cost(&ModelId::OPUS_5_5, &Usage::default()).is_some());
    }

    #[test]
    fn builders_set_cache_read_and_long_context() {
        let r = ModelPricing::from_input_output(4.0, 20.0, 0.01)
            .with_cache_read(0.2)
            .with_long_context(LongContextTier::from_input_output(200_000, 8.0, 40.0));
        approx(r.cache_read_per_mtok, 0.2);
        let tier = r.long_context.unwrap();
        assert_eq!(tier.above_prompt_tokens, 200_000);
        approx(tier.cache_creation_5m_per_mtok, 10.0);
    }

    #[test]
    fn cost_input_and_output_only() {
        // 1M input @ $3/MTok + 0.5M output @ $15/MTok = $3 + $7.5 = $10.5
        let p = PricingTable::default();
        let usage = Usage {
            input_tokens: 1_000_000,
            output_tokens: 500_000,
            ..Usage::default()
        };
        approx(p.cost(&ModelId::SONNET_4_6, &usage), 10.5);
    }

    #[test]
    fn cost_uses_per_ttl_breakdown_when_present() {
        // 1M input + breakdown 1M @ 5m + 1M @ 1h on Sonnet 4.6
        // input: $3, 5m write: $3.75, 1h write: $6, total $12.75
        let p = PricingTable::default();
        let usage = Usage {
            input_tokens: 1_000_000,
            output_tokens: 0,
            cache_creation: Some(CacheCreationBreakdown {
                ephemeral_5m_input_tokens: 1_000_000,
                ephemeral_1h_input_tokens: 1_000_000,
            }),
            ..Usage::default()
        };
        approx(p.cost(&ModelId::SONNET_4_6, &usage), 3.0 + 3.75 + 6.0);
    }

    #[test]
    fn cost_falls_back_to_legacy_cache_field_when_no_breakdown() {
        // Without breakdown we assume 5m TTL: 1M @ $3.75/MTok = $3.75
        let p = PricingTable::default();
        let usage = Usage {
            input_tokens: 0,
            output_tokens: 0,
            cache_creation_input_tokens: Some(1_000_000),
            cache_creation: None,
            ..Usage::default()
        };
        approx(p.cost(&ModelId::SONNET_4_6, &usage), 3.75);
    }

    #[test]
    fn cost_includes_cache_reads() {
        // 1M cache read @ $0.30/MTok = $0.30
        let p = PricingTable::default();
        let usage = Usage {
            cache_read_input_tokens: Some(1_000_000),
            ..Usage::default()
        };
        approx(p.cost(&ModelId::SONNET_4_6, &usage), 0.30);
    }

    #[test]
    fn cost_includes_web_search_requests() {
        let p = PricingTable::default();
        let usage = Usage {
            server_tool_use: Some(ServerToolUseUsage {
                web_search_requests: 50,
            }),
            ..Usage::default()
        };
        approx(p.cost(&ModelId::SONNET_4_6, &usage), 0.50);
    }

    #[test]
    fn breakdown_components_sum_to_total() {
        let p = PricingTable::default();
        let usage = Usage {
            input_tokens: 100_000,
            output_tokens: 50_000,
            cache_creation_input_tokens: Some(20_000),
            cache_read_input_tokens: Some(80_000),
            server_tool_use: Some(ServerToolUseUsage {
                web_search_requests: 3,
            }),
            ..Usage::default()
        };
        let b = p.cost_breakdown(&ModelId::SONNET_4_6, &usage);
        approx(
            b.input + b.output + b.cache_creation + b.cache_read + b.server_tool_use,
            b.total,
        );
    }

    #[test]
    fn unknown_model_returns_zero_cost() {
        let p = PricingTable::default();
        let usage = Usage {
            input_tokens: 1_000_000,
            output_tokens: 1_000_000,
            ..Usage::default()
        };
        let cost = p.cost(&ModelId::custom("claude-future-foo"), &usage);
        approx(cost, 0.0);
    }

    #[test]
    fn custom_table_overrides_bundled_rates() {
        let mut rates = HashMap::new();
        rates.insert(
            ModelId::SONNET_4_6,
            ModelPricing::from_input_output(2.00, 10.00, 0.005),
        );
        let p = PricingTable::custom(rates);
        let usage = Usage {
            input_tokens: 1_000_000,
            ..Usage::default()
        };
        approx(p.cost(&ModelId::SONNET_4_6, &usage), 2.0);
    }

    #[test]
    fn set_inserts_or_replaces_a_single_model() {
        let mut p = PricingTable::default();
        p.set(
            ModelId::SONNET_4_6,
            ModelPricing::from_input_output(99.99, 99.99, 0.0),
        );
        let usage = Usage {
            input_tokens: 1_000_000,
            ..Usage::default()
        };
        approx(p.cost(&ModelId::SONNET_4_6, &usage), 99.99);
    }

    #[test]
    fn from_input_output_derives_cache_multipliers() {
        let r = ModelPricing::from_input_output(10.0, 50.0, 0.01);
        approx(r.cache_creation_5m_per_mtok, 12.5);
        approx(r.cache_creation_1h_per_mtok, 20.0);
        approx(r.cache_read_per_mtok, 1.0);
    }
}
