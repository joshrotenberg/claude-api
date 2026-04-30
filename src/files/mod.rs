//! The Files API (beta).
//!
//! Upload, download, list, and delete files. Once uploaded, file IDs can
//! be referenced in [`DocumentSource::File`](crate::messages::DocumentSource::File)
//! and [`ImageSource::File`](crate::messages::ImageSource::File) blocks
//! instead of base64-encoding the bytes inline on every request.
//!
//! Upload and download both support **true streaming I/O** (no buffering
//! the whole payload in memory) via the `_path` and `_to` variants.
//!
//! # Beta
//!
//! Every Files method automatically sends
//! `anthropic-beta: files-api-2025-04-14`. Override the beta version on the
//! [`Client`](crate::Client) builder if a newer revision is current.

pub mod types;

#[cfg(feature = "async")]
#[cfg_attr(docsrs, doc(cfg(feature = "async")))]
pub mod api;

pub use types::{FileDeleted, FileMetadata, ListFilesParams};

#[cfg(feature = "async")]
pub use api::Files;
