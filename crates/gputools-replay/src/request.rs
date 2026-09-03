//! Fetch request types: the geometry (`Region`/`Size`), the dispatch key
//! (`DispatchUid`), and the per-class request shapes (`TextureRequest`,
//! `WireframeRequest`).
//!
//! The wire-format structs (`GTSize`/`GTPoint3D`/`GTRegion`) and their
//! `Encode` impls are ported verbatim from `probes/src/session.rs:448-490`;
//! `DispatchUid`'s `Encode` impl is ported from `probes/src/session.rs:208-220`.
//! Every `Encode` impl spells out the exact type encoding the runtime reports
//! for the setter it is sent to, so objc2 can re-check it against the runtime
//! in debug builds; a mismatch there is not a type error but a misaligned
//! argument register.

use objc2::encode::{Encode, Encoding};

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
}

/// A texture fetch request. `depth` is fixed to 1 (a 2D texture has one
/// slice; depth 0 returns nothing from the replayer) and is not settable
/// here: it is forced by [`crate::fetch::build_texture_batch`], never taken
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

/// The `dispatchUID` a dispatch-keyed fetch request carries: the ObjC
/// encoding `(?={?=ii}Q)` is an 8-byte UNION, read either as two `int32`s or
/// one `uint64`. It identifies the draw/dispatch whose debug data is being
/// fetched (dossier 00 "The fetch family", item 11).
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DispatchUid(pub u64);

// SAFETY: `DispatchUid` is `#[repr(transparent)]` over a `u64` (8 bytes,
// 8-aligned), and this encoding is the exact `(?={?=ii}Q)` union the setter
// declares, so objc2's runtime encoding check accepts it.
unsafe impl Encode for DispatchUid {
    const ENCODING: Encoding = Encoding::Union(
        "?",
        &[
            Encoding::Struct("?", &[Encoding::Int, Encoding::Int]),
            Encoding::ULongLong,
        ],
    );
}

/// A wireframe (dispatch-keyed) fetch request.
#[derive(Debug, Clone, Copy)]
pub struct WireframeRequest {
    /// The draw/dispatch to render, as a small-integer draw index.
    pub dispatch_uid: DispatchUid,
    /// Solid fill (`true`) vs wireframe lines (`false`).
    pub solid: bool,
}

/// The geometry a texture fetch request carries on the wire. Laid out to
/// match the type encodings the runtime reports for the setters, read off
/// the live class rather than guessed:
///
/// ```text
/// -setSize:    v40@0:8{GTSize=QQQ}16
/// -setRegion:  v64@0:8{GTRegion={GTPoint3D=QQQ}{GTSize=QQQ}}16
/// ```
///
/// A mismatch here is not a type error but a misaligned argument register, so
/// the `Encode` impls below spell the same encodings out exactly and objc2
/// checks them against the runtime in debug builds.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct GTSize {
    pub width: u64,
    pub height: u64,
    pub depth: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct GTPoint3D {
    pub x: u64,
    pub y: u64,
    pub z: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct GTRegion {
    pub origin: GTPoint3D,
    pub size: GTSize,
}

// SAFETY: three `u64`s in declaration order, `#[repr(C)]`, no padding, which
// is exactly `{GTSize=QQQ}`.
unsafe impl Encode for GTSize {
    const ENCODING: Encoding =
        Encoding::Struct("GTSize", &[u64::ENCODING, u64::ENCODING, u64::ENCODING]);
}

// SAFETY: as above, `{GTPoint3D=QQQ}`.
unsafe impl Encode for GTPoint3D {
    const ENCODING: Encoding =
        Encoding::Struct("GTPoint3D", &[u64::ENCODING, u64::ENCODING, u64::ENCODING]);
}

// SAFETY: two `#[repr(C)]` structs of `u64`, so no padding is introduced
// between them either: `{GTRegion={GTPoint3D=QQQ}{GTSize=QQQ}}`.
unsafe impl Encode for GTRegion {
    const ENCODING: Encoding =
        Encoding::Struct("GTRegion", &[GTPoint3D::ENCODING, GTSize::ENCODING]);
}

impl From<Region> for GTRegion {
    /// Converts the public `Region` into the wire struct. `depth` is always
    /// 1, per the hard invariant on [`TextureRequest`]: never taken from the
    /// caller, mirroring `probes::session::build_batch`, which hardcodes
    /// `depth: 1` in the `GTSize` it sends regardless of the request.
    fn from(region: Region) -> Self {
        GTRegion {
            origin: GTPoint3D {
                x: region.origin.x,
                y: region.origin.y,
                z: region.origin.z,
            },
            size: GTSize {
                width: region.size.width,
                height: region.size.height,
                depth: 1,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatch_uid_encoding_matches_the_setter() {
        assert_eq!(DispatchUid::ENCODING.to_string(), "(?={?=ii}Q)");
    }

    #[test]
    fn gtsize_encoding_matches_the_setter() {
        assert_eq!(GTSize::ENCODING.to_string(), "{GTSize=QQQ}");
    }

    #[test]
    fn gtregion_encoding_matches_the_setter() {
        assert_eq!(
            GTRegion::ENCODING.to_string(),
            "{GTRegion={GTPoint3D=QQQ}{GTSize=QQQ}}"
        );
    }

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
        let wire: GTRegion = region.into();
        assert_eq!(wire.origin.x, 1);
        assert_eq!(wire.origin.y, 2);
        assert_eq!(wire.origin.z, 3);
        assert_eq!(wire.size.width, 64);
        assert_eq!(wire.size.height, 64);
        assert_eq!(wire.size.depth, 1);
    }
}
