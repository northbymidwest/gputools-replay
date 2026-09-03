//! Ergonomic domain types over `gputools-replay`: format-aware textures, typed
//! buffers, acceleration-structure geometry, pipeline stages - decoded on
//! demand, with the raw bytes always available and never destroyed.
#![deny(unsafe_op_in_unsafe_fn)]
#![warn(missing_docs)]

pub mod accel;
pub mod buffer;
mod bytes;
pub mod capture;
pub mod describe;
pub mod error;
pub mod format;
pub mod image;
pub mod pipeline;

pub use accel::{Aabb, AccelStructure};
pub use buffer::{Buffer, Heap};
pub use capture::{Aspect, Capture, ManifestStatus};
pub use describe::{DescribedTexture, Descriptions};
pub use error::Error;
pub use gputools_replay::config::ReplayerConfig;
/// The substrate's fetch-request shapes: [`Capture::textures_with`] and
/// [`Capture::wireframes_with`] take slices of [`TextureRequest`] /
/// [`WireframeRequest`] (which itself embeds [`DispatchUid`]), and
/// [`TextureRequest`] takes a [`Region`] (built from [`Point3D`]/[`Size`]) to
/// select a resample region, slice, and level. Re-exported here so a caller
/// of this crate does not also need a direct `gputools-replay` dependency
/// just to name these types.
pub use gputools_replay::request::{
    DispatchUid, Point3D, Region, Size, TextureRequest, WireframeRequest,
};
pub use gputrace_bundle::TextureDescriptor;
pub use image::{Blocks, Texture, Wireframe};
pub use objc2_metal::{MTLPixelFormat, MTLTextureType, MTLTextureUsage};
pub use pipeline::{Pipeline, Stage, StageKind, Stats};
