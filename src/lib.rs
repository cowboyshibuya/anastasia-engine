#![allow(
    unknown_lints,
    clippy::collapsible_match,
    clippy::manual_checked_ops,
    clippy::unnecessary_sort_by,
    clippy::useless_conversion
)]

//! Root `anastasia` crate: the entrypoint + cli layer on top of the `anastasia-tui`
//! presentation crate (which in turn re-exports `anastasia-app-core` and
//! `anastasia-base`).
//!
//! The presentation modules (`tui`, `video_export`) live in `anastasia-tui` and the
//! non-presentation modules live in `anastasia-app-core`; both are re-exported here
//! via `pub use anastasia_tui::*`, so existing `crate::<module>` paths (e.g.
//! `crate::config`, `crate::server`, `crate::tui`) keep resolving unchanged
//! across the cli code that was not moved.

// Re-export the presentation layer (and, transitively, the application core)
// so `crate::tui`, `crate::video_export`, and `crate::<app-core module>` paths
// resolve.
pub use anastasia_tui::*;

// Cli + entrypoint layer (kept in the root crate).
pub mod cli;

use anyhow::Result;

pub async fn run() -> Result<()> {
    cli::startup::run().await
}
