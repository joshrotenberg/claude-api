//! Tool-dispatch foundation: the [`Tool`] trait, plus a registry and agent
//! loop runner (the latter two land in v0.2 tasks #19 and #20).
//!
//! v0.2 will layer three ergonomic shapes on this single trait:
//!
//! 1. [`Tool`] -- async trait. Implement directly when each tool is its
//!    own type (typical for shared crate code).
//! 2. `ToolRegistry::register` -- closure-based handler. The registry wraps
//!    closures in an internal `FnTool<F>` adapter that implements [`Tool`].
//! 3. `ToolRegistry::register_typed::<Args>` -- typed-input handler driven
//!    by `schemars`. Internally builds a `Tool` impl that deserializes the
//!    raw input into `Args` before dispatching. Behind the
//!    `schemars-tools` feature.
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
pub use tool::{fn_approver, ApprovalDecision, Tool, ToolApprover, ToolError};

#[cfg(feature = "conversation")]
pub use runner::RunOptions;

#[cfg(feature = "schemars-tools")]
pub use typed::TypedTool;
