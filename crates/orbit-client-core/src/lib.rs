//! Embedded Multi-Host Rust Client Core for Tauri Desktop, iOS (SwiftUI), Android (Compose), HarmonyOS 7+ (ArkUI), Web (WASM/PWA), and `orbit` CLI.
//!
//! Provides:
//! - Concurrent Multi-Host fleet management (Devbox, Linux workstation, Mac mini)
//! - Happy-Eyeballs + Dedicated SSH Tunnel + Noise NK E2EE Relay smart routing with 10s Ping keepalive
//! - Zstd wire compression + zero-copy binary terminal framing
//! - `0ms` instant-open local SQLite WAL replica cache with monotonic `since_seq` catch-up
//! - Pure-Rust streaming Markdown AST, `syntect` syntax highlighting, Git diff model, and VT100 terminal grid

#[cfg(feature = "native")]
pub mod app_core;
pub mod catalog;
#[cfg(feature = "native")]
pub mod cloud_sync;
pub mod composer;
pub mod contract;
#[cfg(feature = "native")]
pub mod desktop_ops;
pub mod diagram;
pub mod export;
#[cfg(feature = "native")]
pub mod logging;
pub mod math;
pub mod render;
pub mod sidebar;
pub mod stage;
pub mod subagents;

pub mod viewmodel;
pub use viewmodel::{HostConnectionState, HostRouteMode, SavedHostProfile};

#[cfg(feature = "native")]
mod native;
#[cfg(feature = "native")]
pub use native::*;
