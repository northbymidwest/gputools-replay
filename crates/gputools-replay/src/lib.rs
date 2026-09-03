//! Safe interface to Apple's private GPUToolsReplay framework.
//!
//! This is the faithful substrate layer: it exposes the replayer's surfaces
//! (fetch, playback, harvester) with typed results that preserve every field.
//! Ergonomic domain types belong in a downstream crate built on this one.
//!
//! The public API is safe by default: only [`Session::configure_env`] is
//! `unsafe`, an opt-in step for applying non-default replayer configuration,
//! with its precondition documented on that fn. Every other public fn is
//! safe to call. Internally the crate contains audited `unsafe` (Objective-C
//! message sends and C FFI), each block justified by the reverse-engineering
//! evidence in `docs/findings/`.
#![deny(unsafe_op_in_unsafe_fn)]
#![warn(missing_docs)]

pub mod config;
pub mod error;
mod fetch;
pub mod harvester;
mod objc;
pub mod reply;
pub mod request;
mod session;
mod util;

pub use config::ReplayerConfig;
pub use error::{FetchError, HarvesterError, SessionError};
pub use session::Session;
