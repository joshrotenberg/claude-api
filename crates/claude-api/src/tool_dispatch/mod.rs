//! Tool-dispatch: the [`Tool`] trait, registry, typed inputs, and the agent
//! loop runner.
//!
//! Three ergonomic shapes for registering tools:
//!
//! 1. **[`Tool`] trait** -- implement directly when each tool is its own type.
//!    Gives full control over the JSON Schema, the async handler, and errors.
//! 2. **[`ToolRegistry::register`]** -- closure-based handler. The registry
//!    wraps closures in an internal `FnTool<F>` adapter. Convenient for
//!    one-off tools that don't need a dedicated type.
//! 3. **[`TypedTool`] + `ToolRegistry::register_typed`** -- typed-input
//!    handler driven by `schemars`. The input JSON is deserialized into a
//!    concrete `Args` type before dispatch. Behind the `schemars-tools`
//!    feature (or use `#[derive(Tool)]` from `claude-api-derive` for the
//!    most ergonomic form).
//!
//! The agent loop lives in [`runner`] (feature `conversation`). It drives
//! repeated `messages.create` calls, dispatching tools in parallel via
//! [`Tool::invoke`], optionally enforcing a cost budget and a mid-stream
//! approval gate.
//!
//! Gated on the `async` feature.

pub mod registry;
pub mod tool;

#[cfg(feature = "conversation")]
#[cfg_attr(docsrs, doc(cfg(feature = "conversation")))]
pub mod runner;

#[cfg(feature = "schemars-tools")]
#[cfg_attr(docsrs, doc(cfg(feature = "schemars-tools")))]
pub mod typed;

pub use registry::{FnTool, ToolRegistry};
pub use tool::{ApprovalDecision, Tool, ToolApprover, ToolError, fn_approver};

#[cfg(feature = "conversation")]
pub use runner::RunOptions;

#[cfg(feature = "schemars-tools")]
pub use typed::TypedTool;
