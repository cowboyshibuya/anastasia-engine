//! Rust SDK for the anastasia harness.
//!
//! The API crate (`anastasia-harness-api`) defines the wire protocol. This crate
//! is what you actually build a client with: connect, drive sessions, stream
//! events, and get told why the connection died in a sentence a user can act
//! on.
//!
//! It is the Rust counterpart of `sdk/typescript`, and deliberately mirrors it
//! capability for capability. Desktop2 is built on this crate rather than on
//! the raw protocol, so the SDK's design is exercised by a real, shipping
//! client every day rather than only by its own examples.
//!
//! ```no_run
//! use anastasia_sdk::{ConnectOptions, AnastasiaClient, RunOptions};
//!
//! let client = AnastasiaClient::connect(ConnectOptions::default())?;
//! let session = client.create_session(None)?;
//! let turn = client.run(&session.session_id, "what is 2 + 2?", RunOptions::default())?;
//! println!("{}", turn.text);
//! # Ok::<(), anastasia_sdk::Error>(())
//! ```
//!
//! Connect to a remote user's persistent harness through system OpenSSH:
//! ```no_run
//! use anastasia_sdk::{AnastasiaClient, SshConnectOptions};
//! let client = AnastasiaClient::connect_ssh(SshConnectOptions::new("my-ssh-alias"))?;
//! let sessions = client.list_sessions()?;
//! # Ok::<(), anastasia_sdk::Error>(())
//! ```
//! This uses existing SSH config, keys, agent, and known_hosts. Verify new host
//! keys with SSH before connecting. The remote needs a release supporting
//! `anastasia api --stdio`, which is never installed or updated by this SDK. A POSIX
//! remote shell is required. Dropping the final client clone kills and reaps its
//! SSH child, not the remote shared daemon. `connect_timeout` bounds startup and
//! hello independently of the ordinary request timeout.

mod client;
mod diagnostics;
mod errors;
mod launch;
mod ssh;
mod structured;

#[cfg(test)]
#[path = "sdk_tests/parity.rs"]
mod parity_tests;

pub use client::{
    AnastasiaClient, ConnectOptions, EventStream, FileContent, FileStatus, GlobalEventStream,
    GlobalEventsOptions, RunOptions, RuntimeInfo, SearchTextOptions, ToolCall, Transport,
    TurnResult, UnixTransport, Usage,
};
pub use diagnostics::{SocketState, Stage, describe_disconnect, explain, human_duration};
pub use errors::{Error, ErrorKind, Result};
pub use launch::{
    LaunchOptions, LaunchedInstance, WakeMode, ensure_runtime, inherit_credentials,
    launch_instance, socket_accepts, user_anastasia_home, user_app_config_dir, wait_for_socket,
};
pub use ssh::SshConnectOptions;
pub use structured::{
    RunStructuredError, RunStructuredOptions, StructuredEventCallback, StructuredOutputAttempt,
    StructuredOutputError, StructuredOutputSchema, StructuredSchemaError, StructuredTurnResult,
    StructuredValidationIssue,
};

/// The protocol types, re-exported so a client needs one dependency, not two.
pub use anastasia_harness_api as api;
pub use anastasia_harness_api::{
    ApiEvent, ApiRequest, HistoryMessage, HistoryToolCall, ModelRouteInfo, PermissionDecision,
    RenderedImage, RenderedImageAnchor, RenderedImageSource, SessionInfo, TextMatch, UserAnswers,
    UserQuestion, UserQuestionOption, api_socket_path,
};
