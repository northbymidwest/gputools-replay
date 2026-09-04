//! The authoritative texture descriptor, read off the live `MTLTexture` the
//! replayer created for a streamRef (see [`crate::Session::texture_descriptor`]).
//!
//! Fields are raw Metal enum/bitflag values (formats stay `u32`; a domain crate
//! interprets them), sourced from the loaded object rather than the static
//! store - so they are correct across capture serialization schemas and need no
//! store0-offset ordinal join. See
//! `docs/design/2026-09-04-session-descriptors-and-ffi-layering.md`.

use objc2::runtime::ProtocolObject;
use objc2_metal::MTLTexture;

/// A texture's descriptor as the replayer holds it live, keyed by `stream_ref`.
/// `pixel_format` is an `MTLPixelFormat` code, `texture_type` an
/// `MTLTextureType`, `usage` an `MTLTextureUsage` bitmask.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextureDescriptor {
    /// The streamRef this texture answers to (the fetch key and the map key).
    pub stream_ref: u64,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Depth in pixels (1 for 2D).
    pub depth: u32,
    /// The `MTLPixelFormat` code.
    pub pixel_format: u32,
    /// The `MTLTextureType` code.
    pub texture_type: u32,
    /// Number of mip levels.
    pub mip_levels: u32,
    /// Number of array slices (1 when not an array).
    pub array_length: u32,
    /// MSAA sample count (1 when not multisampled).
    pub sample_count: u32,
    /// The `MTLTextureUsage` bitmask.
    pub usage: u64,
}

impl TextureDescriptor {
    /// Read the descriptor off a live texture via the public Metal API. Every
    /// field is a `+0` NSUInteger property; no field is guessed.
    pub(crate) fn from_texture(stream_ref: u64, tex: &ProtocolObject<dyn MTLTexture>) -> Self {
        TextureDescriptor {
            stream_ref,
            width: tex.width() as u32,
            height: tex.height() as u32,
            depth: tex.depth() as u32,
            pixel_format: tex.pixelFormat().0 as u32,
            texture_type: tex.textureType().0 as u32,
            mip_levels: tex.mipmapLevelCount() as u32,
            array_length: tex.arrayLength() as u32,
            sample_count: tex.sampleCount() as u32,
            usage: tex.usage().0 as u64,
        }
    }
}
