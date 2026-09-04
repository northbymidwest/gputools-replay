//! Fetch request types: the geometry (`Region`/`Size`), the dispatch key
//! (`DispatchUid`, re-exported from `-sys`), and the per-class request shapes
//! (`TextureRequest`, `WireframeRequest`).
//!
//! The wire-format structs (`GTSize`/`GTPoint3D`/`GTRegion`) and `DispatchUid`,
//! with the `Encode` impls that spell out the exact type encodings the runtime
//! reports, are raw FFI and live in `gputools_replay_sys::replay`. This module
//! maps the public geometry onto them ([`Region::to_gt`]).

pub use gputools_replay_sys::replay::DispatchUid;
use gputools_replay_sys::replay::{GTPoint3D, GTRegion, GTSize};

/// A 3D origin point. Mirrors the sys `GTPoint3D` (`{GTPoint3D=QQQ}`).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Point3D {
    /// X coordinate.
    pub x: u64,
    /// Y coordinate.
    pub y: u64,
    /// Z coordinate.
    pub z: u64,
}

/// A 2D size, the dimensions a fetch resamples a texture into. Mirrors the
/// caller-controllable fields of the sys `GTSize`; depth is a hard, never
/// caller-settable invariant (see [`crate::request::TextureRequest`]) and is
/// not part of this type.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Size {
    /// Width in texels.
    pub width: u64,
    /// Height in texels.
    pub height: u64,
}

/// The region a texture fetch resamples into. **The fetch resamples**: it
/// scales the texture to fit the requested region, preserving aspect ratio,
/// so asking for a size that is not the texture's own returns resampled
/// pixels, not a crop and not the natural image (measured, probes spike
/// round 6).
///
/// Zero is not empty: [`Region::ZERO`] (a zero origin, zero size) returns each
/// texture at its **natural** size, unresampled (measured on
/// `small.gputrace`: a zero-region sweep returned the same dimensions
/// `gpudebug` reports for the same textures).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Region {
    /// The region's origin.
    pub origin: Point3D,
    /// The region's size.
    pub size: Size,
}

impl Region {
    /// A zero origin, zero size: natural size, unresampled.
    pub const ZERO: Region = Region {
        origin: Point3D { x: 0, y: 0, z: 0 },
        size: Size {
            width: 0,
            height: 0,
        },
    };

    /// Converts the public `Region` into the sys wire struct. `depth` is
    /// always 1, per the hard invariant on [`TextureRequest`]: never taken
    /// from the caller, mirroring `probes::session::build_batch`, which
    /// hardcodes `depth: 1` in the `GTSize` it sends regardless of the request.
    pub(crate) fn to_gt(self) -> GTRegion {
        GTRegion {
            origin: GTPoint3D {
                x: self.origin.x,
                y: self.origin.y,
                z: self.origin.z,
            },
            size: GTSize {
                width: self.size.width,
                height: self.size.height,
                depth: 1,
            },
        }
    }
}

/// A texture fetch request. `depth` is fixed to 1 (a 2D texture has one
/// slice; depth 0 returns nothing from the replayer) and is not settable
/// here: it is forced by `fetch::build_texture_batch`, never taken
/// from the caller, mirroring `probes::session::build_batch`.
#[derive(Debug, Clone)]
pub struct TextureRequest {
    /// The replayer's own key for the texture. Sparse: a capture's refs are
    /// nothing like `0..n`, so callers sweep a range and read back what
    /// answered.
    pub stream_ref: u64,
    /// The region to fetch; [`Region::ZERO`] means natural size, unresampled.
    pub region: Region,
    /// The texture plane (0 for non-planar).
    pub plane: u32,
    /// The array slice (or cube face) to fetch. 0 for a non-array texture.
    pub slice: u32,
    /// The mip level to fetch. 0 for the base level.
    pub level: u32,
}

impl TextureRequest {
    /// A natural-size (unresampled), plane-0, slice-0, level-0 request for
    /// `stream_ref`.
    pub fn natural(stream_ref: u64) -> Self {
        Self {
            stream_ref,
            region: Region::ZERO,
            plane: 0,
            slice: 0,
            level: 0,
        }
    }

    /// A fully specified request. `slice` and `level` default to 0; use
    /// [`TextureRequest::with_slice_level`] to set them.
    pub fn new(stream_ref: u64, region: Region, plane: u32) -> Self {
        Self {
            stream_ref,
            region,
            plane,
            slice: 0,
            level: 0,
        }
    }

    /// Returns `self` with `slice` and `level` set, for array/mip fetches.
    pub fn with_slice_level(mut self, slice: u32, level: u32) -> Self {
        self.slice = slice;
        self.level = level;
        self
    }
}

/// A wireframe (dispatch-keyed) fetch request.
#[derive(Debug, Clone, Copy)]
pub struct WireframeRequest {
    /// The draw/dispatch to render, as a small-integer draw index.
    pub dispatch_uid: DispatchUid,
    /// Solid fill (`true`) vs wireframe lines (`false`).
    pub solid: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_is_zero_region_plane_zero() {
        let r = TextureRequest::natural(42);
        assert_eq!(r.stream_ref, 42);
        assert_eq!(r.region, Region::ZERO);
        assert_eq!(r.plane, 0);
        assert_eq!(r.slice, 0);
        assert_eq!(r.level, 0);
    }

    #[test]
    fn with_slice_level_sets_both_fields() {
        let r = TextureRequest::natural(1).with_slice_level(1, 3);
        assert_eq!(r.slice, 1);
        assert_eq!(r.level, 3);
    }

    #[test]
    fn region_conversion_forces_depth_one() {
        let region = Region {
            origin: Point3D { x: 1, y: 2, z: 3 },
            size: Size {
                width: 64,
                height: 64,
            },
        };
        let wire = region.to_gt();
        assert_eq!(wire.origin.x, 1);
        assert_eq!(wire.origin.y, 2);
        assert_eq!(wire.origin.z, 3);
        assert_eq!(wire.size.width, 64);
        assert_eq!(wire.size.height, 64);
        assert_eq!(wire.size.depth, 1);
    }
}
