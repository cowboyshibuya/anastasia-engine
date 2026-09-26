//! Logging infrastructure for anastasia
//!
//! Logs to ~/.anastasia-cli/logs/ with automatic rotation
//!
//! Supports thread-local context for server, session, provider, and model info.
//!
//! The implementation lives in the `anastasia-logging` workspace crate so that this
//! very-high-fanout, low-churn subsystem forms a stable compile-cache boundary
//! and does not pull the root crate into rebuilds. This module is a thin facade
//! that preserves the existing `crate::logging::*` API for all call sites.

pub use anastasia_logging::*;
