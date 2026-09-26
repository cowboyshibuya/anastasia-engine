#![allow(
    unknown_lints,
    clippy::collapsible_match,
    clippy::manual_checked_ops,
    clippy::unnecessary_sort_by,
    clippy::useless_conversion
)]

//! Presentation layer for anastasia (terminal UI + offline replay export).
//!
//! This crate holds the `tui` and `video_export` modules that were extracted
//! out of the monolithic root `anastasia` crate so they compile as a separate
//! rustc unit. The application core it builds on (server, agent, provider,
//! auth, session, tool, config, ...) lives in `anastasia-app-core` and is
//! re-exported here via `pub use anastasia_app_core::*`, so every existing
//! `crate::<module>` path (e.g. `crate::config`, `crate::server`) keeps
//! resolving unchanged across the tui code. The root `anastasia` crate (cli + bin)
//! re-exports this crate via `pub use anastasia_tui::*`.

// Application core: re-export every `anastasia-app-core` module (which itself
// re-exports `anastasia-base`) so `crate::<module>` paths resolve here exactly as
// they did before the split.
pub use anastasia_app_core::*;

// Presentation layer (kept in this crate).
pub mod tui;
pub mod video_export;
