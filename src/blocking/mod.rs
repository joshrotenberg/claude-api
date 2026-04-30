//! Blocking (synchronous) variants of the HTTP surface.
//!
//! Mirrors the async API in [`crate::client`] / [`crate::messages::api`] /
//! [`crate::models`] but uses [`reqwest::blocking`] under the hood, so no
//! tokio runtime is required.
//!
//! Gated on the `sync` feature.
//!
//! ```no_run
//! use claude_api::blocking::Client;
//! use claude_api::messages::CreateMessageRequest;
//! use claude_api::types::ModelId;
//!
//! let client = Client::new(std::env::var("ANTHROPIC_API_KEY").unwrap());
//! let req = CreateMessageRequest::builder()
//!     .model(ModelId::SONNET_4_6)
//!     .max_tokens(64)
//!     .user("hi")
//!     .build()
//!     .unwrap();
//! let resp = client.messages().create(req).unwrap();
//! # let _ = resp;
//! ```
//!
//! # Scope
//!
//! v0.2 ships sync support for the **transport layer only**: [`Client`],
//! [`Messages`], [`Models`]. Higher-level helpers (`Conversation`,
//! `ToolRegistry`, `Client::run`, streaming) remain async-only. Most apps
//! that want sync are looking for "I just need a simple HTTP client" --
//! reach for the async path if you need the multi-turn or agent helpers.

#![cfg(feature = "sync")]

mod client;
mod messages;
mod models;

pub use client::{Client, ClientBuilder};
pub use messages::Messages;
pub use models::Models;
