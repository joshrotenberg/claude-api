//! Type-safe Rust client for the Anthropic API.
//!
//! See the [crate README](https://github.com/joshrotenberg/anthropic-rs) for an overview.

#![cfg_attr(docsrs, feature(doc_cfg))]

#[allow(dead_code)]
pub(crate) const ANTHROPIC_VERSION: &str = "2023-06-01";
#[allow(dead_code)]
pub(crate) const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
#[allow(dead_code)]
pub(crate) const USER_AGENT: &str = concat!("claude-api-rs/", env!("CARGO_PKG_VERSION"));

pub mod auth;
pub mod error;

#[cfg(feature = "async")]
#[cfg_attr(docsrs, doc(cfg(feature = "async")))]
pub mod client;

#[cfg(feature = "sync")]
#[cfg_attr(docsrs, doc(cfg(feature = "sync")))]
pub mod blocking;

#[cfg(any(feature = "async", feature = "sync"))]
#[cfg_attr(docsrs, doc(cfg(any(feature = "async", feature = "sync"))))]
pub mod retry;

pub(crate) mod forward_compat;

#[cfg(feature = "streaming")]
#[cfg_attr(docsrs, doc(cfg(feature = "streaming")))]
pub mod sse;

pub mod pagination;

#[cfg(feature = "pricing")]
#[cfg_attr(docsrs, doc(cfg(feature = "pricing")))]
pub mod pricing;

#[cfg(feature = "conversation")]
#[cfg_attr(docsrs, doc(cfg(feature = "conversation")))]
pub mod conversation;

#[cfg(feature = "async")]
#[cfg_attr(docsrs, doc(cfg(feature = "async")))]
pub mod tool_dispatch;

pub mod types;

pub mod messages;
pub mod models;

#[cfg(feature = "async")]
pub use client::{Client, ClientBuilder};
pub use error::{Error, Result};
