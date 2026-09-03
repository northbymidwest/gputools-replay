//! The descriptor join: a deterministic creation-order, best-effort zip of
//! fetched streamRefs against manifest descriptors.
//!
//! The fetch reply carries no key back into the manifest (MEASURED: the reply
//! record holds only streamRef + geometry; dossier 00). Both sides are instead
//! enumerable in resource-creation order - streamRefs ascending, descriptors by
//! `store0_offset` ascending - and correspond rank-for-rank (validated across
//! and within dims runs). So this zips positionally; dims+format are a hard
//! check on each pairing, never a soft matcher. A descriptor the walk cannot
//! place lands in [`Descriptions::unplaced`] rather than erroring.
//!
//! Known limitation: the walk catches a COUNT imbalance - a descriptor that
//! cannot be placed, reported via `unplaced` - not a same-geometry
//! one-position SHIFT. If an undescribed fetched texture shares exact
//! `(width, height, format)` with described textures and sorts earlier by
//! streamRef, attribution can shift by one silently (a texture ends up
//! attributed to the wrong, geometrically-identical descriptor rather than
//! left unattributed). This is inherent to the measured positional/ordinal
//! model (validated 208 matches, 0 violations, under
//! `force_load_unused_resources` plus a full sweep, dossier 00), not a
//! defect. A base `Depth32Float` (252) and a combined resource's depth aspect
//! (also 252) are geometrically indistinguishable and are one instance of
//! this.

use gputrace_bundle::TextureDescriptor;
use objc2_metal::{MTLTextureType, MTLTextureUsage};

use crate::Texture;

/// A fetched texture paired with its manifest descriptor.
pub struct DescribedTexture {
    /// The fetched texture (its pixel bytes are always authoritative).
    pub texture: Texture,
    /// The joined descriptor, or `None` only when this fetched texture has no
    /// manifest descriptor (a runtime/aspect texture absent from the bundle).
    pub descriptor: Option<TextureDescriptor>,
}

impl DescribedTexture {
    /// The texture's mip level count, if the manifest describes it.
    pub fn mip_count(&self) -> Option<u32> {
        self.descriptor.map(|d| d.mip_levels)
    }
    /// The texture's array length, if the manifest describes it.
    pub fn array_len(&self) -> Option<u32> {
        self.descriptor.map(|d| d.array_length)
    }
    /// The texture's `MTLTextureType`, if the manifest describes it (wraps
    /// the descriptor's raw `texture_type`).
    pub fn texture_type(&self) -> Option<MTLTextureType> {
        self.descriptor.map(|d| MTLTextureType(d.texture_type as _))
    }
    /// The texture's `MTLTextureUsage` bitflags, if the manifest describes it
    /// (wraps the descriptor's raw `usage`).
    pub fn usage(&self) -> Option<MTLTextureUsage> {
        self.descriptor.map(|d| MTLTextureUsage(d.usage as _))
    }
}

/// The result of joining fetched textures against manifest descriptors, from
/// [`crate::Capture::describe`].
pub struct Descriptions {
    /// Aligned to the input `&[Texture]`: each fetched texture's manifest
    /// descriptor, or `None` if the manifest does not attribute it.
    pub per_texture: Vec<Option<TextureDescriptor>>,
    /// Manifest descriptors that no fetched texture claimed.
    pub unplaced: Vec<TextureDescriptor>,
    /// Manifest descriptors the join skipped as combined depth+stencil (the
    /// fetch never serves the combined format directly, see
    /// [`crate::format::FormatKind::is_combined_depth_stencil`]). Every
    /// descriptor is in exactly one of `per_texture` (as a matched `Some`),
    /// `unplaced`, or `transparent`, so `(count of Some in per_texture) +
    /// unplaced.len() + transparent.len() == descs.len()`.
    pub transparent: Vec<TextureDescriptor>,
}

/// Zip `keys` (the `(width, height, format)` of each fetched texture, sorted by
/// streamRef ascending, deduplicated) against `descs` (sorted by
/// `store0_offset` ascending). Returns, per key, the index into `descs` it
/// pairs with (`None` for an extra key); the indices into `descs` of every
/// non-combined descriptor the walk never placed; and the indices into
/// `descs` of every combined depth+stencil descriptor (always skipped, never
/// placed - see [`Descriptions::transparent`]). These three partition all of
/// `descs`: every index is placed exactly once, unplaced, or transparent.
pub(crate) fn zip_best_effort(
    keys: &[(u32, u32, u32)],
    descs: &[TextureDescriptor],
) -> (Vec<Option<usize>>, Vec<usize>, Vec<usize>) {
    let combined =
        |d: &TextureDescriptor| crate::format::format_kind(d.format).is_combined_depth_stencil();
    let mut out = vec![None; keys.len()];
    let mut placed = vec![false; descs.len()];
    let mut j = 0usize;
    for (i, &(w, h, f)) in keys.iter().enumerate() {
        while descs.get(j).is_some_and(combined) {
            j += 1; // combined depth-stencil descriptors are transparent
        }
        if let Some(d) = descs.get(j)
            && d.width == w
            && d.height == h
            && d.format == f
        {
            out[i] = Some(j);
            placed[j] = true;
            j += 1;
        }
    }
    // `combined` descriptors are never placed (the loop above always skips
    // past them before matching), so these two filters partition the
    // remaining, non-transparent descriptors from the transparent ones.
    let unplaced = descs
        .iter()
        .enumerate()
        .filter(|(idx, d)| !combined(d) && !placed[*idx])
        .map(|(idx, _)| idx)
        .collect();
    let transparent = descs
        .iter()
        .enumerate()
        .filter(|(_, d)| combined(d))
        .map(|(idx, _)| idx)
        .collect();
    (out, unplaced, transparent)
}

/// Join `texs` (already fetched, in any order) against `descs` (the
/// manifest's descriptors, `store0_offset`-ordered; empty means the manifest
/// is absent/unparseable/empty). Pure and infallible: a gap in either
/// direction is reported through the returned [`Descriptions`], never an
/// error.
pub(crate) fn describe_textures(texs: &[Texture], descs: &[TextureDescriptor]) -> Descriptions {
    if descs.is_empty() {
        return Descriptions {
            per_texture: vec![None; texs.len()],
            unplaced: Vec::new(),
            transparent: Vec::new(),
        };
    }

    // Order fetched textures by streamRef ascending, keeping the first of
    // any duplicate streamRef (the fetch can emit a ref twice).
    let mut order: Vec<usize> = (0..texs.len()).collect();
    order.sort_by_key(|&i| texs[i].stream_ref());
    let mut seen = std::collections::HashSet::new();
    let sorted: Vec<usize> = order
        .into_iter()
        .filter(|&i| seen.insert(texs[i].stream_ref()))
        .collect();

    let keys: Vec<(u32, u32, u32)> = sorted
        .iter()
        .map(|&i| (texs[i].width(), texs[i].height(), texs[i].format().0 as u32))
        .collect();
    let (matched, unplaced_idx, transparent_idx) = zip_best_effort(&keys, descs);

    // Map each fetched texture (by its original index) to its descriptor.
    let mut desc_for: Vec<Option<TextureDescriptor>> = vec![None; texs.len()];
    for (si, &ti) in sorted.iter().enumerate() {
        desc_for[ti] = matched[si].map(|dj| descs[dj]);
    }

    Descriptions {
        per_texture: desc_for,
        unplaced: unplaced_idx.into_iter().map(|dj| descs[dj]).collect(),
        transparent: transparent_idx.into_iter().map(|dj| descs[dj]).collect(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::bytes::{AlignedBuf, Payload};
    use gputrace_bundle::TextureDescriptor;

    fn d(off: u64, w: u32, h: u32, fmt: u32, mip: u32) -> TextureDescriptor {
        TextureDescriptor {
            store0_offset: off,
            format: fmt,
            texture_type: 2,
            width: w,
            height: h,
            depth: 1,
            mip_levels: mip,
            array_length: 1,
            sample_count: 1,
            usage: 0,
            texture_id: 0,
        }
    }

    // A BGRA8 Texture with no payload content of interest, for wrapping in a
    // DescribedTexture or feeding to `describe_textures` (the typed
    // accessors only touch `descriptor`; `describe_textures` keys on
    // stream_ref/width/height/format).
    fn tex() -> Texture {
        sr_tex(0, 1, 1)
    }

    fn sr_tex(stream_ref: u64, width: u32, height: u16) -> Texture {
        let buf = Arc::new(AlignedBuf::from_bytes(&[0u8; 4]));
        Texture::for_test(stream_ref, width, height, 4, 80, Payload::new(buf, 0, 4))
    }

    #[test]
    fn typed_texture_type_and_usage_wrap_the_raw_descriptor_fields() {
        // texture_type=8 (Cube's raw MTLTextureType value) and a synthetic
        // usage bit pattern; the accessors must wrap them as typed enums,
        // not hand back the raw integers.
        let desc = TextureDescriptor {
            texture_type: 8,
            usage: 0b0101,
            ..d(0, 1, 1, 80, 1)
        };
        let described = DescribedTexture {
            texture: tex(),
            descriptor: Some(desc),
        };
        assert_eq!(described.texture_type(), Some(MTLTextureType(8)));
        assert_eq!(described.usage(), Some(MTLTextureUsage(0b0101)));

        let bare = DescribedTexture {
            texture: tex(),
            descriptor: None,
        };
        assert_eq!(bare.texture_type(), None);
        assert_eq!(bare.usage(), None);
    }

    #[test]
    fn zips_in_order_including_intra_run() {
        // three same-dims descriptors (offset order = mip 1,3,7) vs three refs.
        let descs = [
            d(505, 64, 64, 80, 1),
            d(575, 64, 64, 80, 3),
            d(666, 64, 64, 80, 7),
        ];
        let keys = [(64, 64, 80), (64, 64, 80), (64, 64, 80)];
        let (got, unplaced, transparent) = zip_best_effort(&keys, &descs);
        assert_eq!(got, vec![Some(0), Some(1), Some(2)]);
        assert!(unplaced.is_empty());
        assert!(transparent.is_empty());
    }

    #[test]
    fn trailing_extra_ref_gets_none() {
        // 3 descriptors, 4 refs; the extra 64x64 ref trails and gets None.
        let descs = [
            d(1, 32, 32, 80, 1),
            d(2, 64, 64, 80, 1),
            d(3, 64, 64, 80, 1),
        ];
        let keys = [(32, 32, 80), (64, 64, 80), (64, 64, 80), (64, 64, 80)];
        let (got, unplaced, transparent) = zip_best_effort(&keys, &descs);
        assert_eq!(got, vec![Some(0), Some(1), Some(2), None]);
        assert!(unplaced.is_empty());
        assert!(transparent.is_empty());
    }

    #[test]
    fn unplaceable_descriptor_lands_in_unplaced_not_an_error() {
        // 2 descriptors but only 1 matching ref: the second descriptor is
        // never placed, so it lands in `unplaced` - no error.
        let descs = [d(1, 64, 64, 80, 1), d(2, 64, 64, 80, 1)];
        let keys = [(64, 64, 80)];
        let (got, unplaced, transparent) = zip_best_effort(&keys, &descs);
        assert_eq!(got, vec![Some(0)]);
        assert_eq!(unplaced, vec![1]);
        assert!(transparent.is_empty());
    }

    #[test]
    fn combined_ds_descriptors_are_transparent() {
        // a color descriptor plus two combined depth-stencil (260) descriptors;
        // the fetch never serves the combined format, so both 260s are skipped,
        // never required to place, and never counted as unplaced.
        let descs = [
            d(1, 64, 64, 80, 1),
            d(2, 64, 64, 260, 1),
            d(3, 64, 64, 260, 1),
        ];
        let keys = [(64, 64, 80), (64, 64, 252), (64, 64, 252)];
        let (got, unplaced, transparent) = zip_best_effort(&keys, &descs);
        assert_eq!(got, vec![Some(0), None, None]);
        assert!(unplaced.is_empty());
        assert_eq!(transparent, vec![1, 2]);
    }

    #[test]
    fn combined_ds_interspersed_is_skipped() {
        // color, combined (260), color, all same dims; the trailing color still
        // places past the skipped combined descriptor.
        let descs = [
            d(1, 64, 64, 80, 1),
            d(2, 64, 64, 260, 1),
            d(3, 64, 64, 80, 1),
        ];
        let keys = [(64, 64, 80), (64, 64, 252), (64, 64, 80)];
        let (got, unplaced, transparent) = zip_best_effort(&keys, &descs);
        assert_eq!(got, vec![Some(0), None, Some(2)]);
        assert!(unplaced.is_empty());
        assert_eq!(transparent, vec![1]);
    }

    #[test]
    fn describe_textures_puts_an_unmatched_descriptor_in_unplaced() {
        // One fetched texture matches the first descriptor; the second
        // descriptor (different dims) matches nothing fetched and lands in
        // `unplaced`, not an error.
        let descs = [d(1, 64, 64, 80, 1), d(2, 32, 32, 80, 1)];
        let texs = [sr_tex(1, 64, 64)];
        let got = describe_textures(&texs, &descs);
        assert_eq!(got.per_texture, vec![Some(descs[0])]);
        assert_eq!(got.unplaced, vec![descs[1]]);
        assert!(got.transparent.is_empty());
    }

    #[test]
    fn describe_textures_combined_ds_never_lands_in_unplaced() {
        // A combined depth-stencil descriptor has no matching fetched
        // texture (the fetch never serves the combined format directly) but
        // stays transparent: not attributed, not unplaced either.
        let descs = [d(1, 64, 64, 80, 1), d(2, 64, 64, 260, 1)];
        let texs = [sr_tex(1, 64, 64)];
        let got = describe_textures(&texs, &descs);
        assert_eq!(got.per_texture, vec![Some(descs[0])]);
        assert!(got.unplaced.is_empty());
        assert_eq!(got.transparent, vec![descs[1]]);
    }

    #[test]
    fn describe_textures_partitions_matched_unplaced_and_transparent() {
        // A color + two combined-DS (260) + one unmatched single-plane
        // descriptor; only the color has a matching fetched texture.
        let descs = [
            d(1, 64, 64, 80, 1),  // color: matched
            d(2, 64, 64, 260, 1), // combined DS: transparent
            d(3, 64, 64, 260, 1), // combined DS: transparent
            d(4, 32, 32, 80, 1),  // unmatched single-plane: unplaced
        ];
        let texs = [sr_tex(1, 64, 64)];
        let got = describe_textures(&texs, &descs);
        assert_eq!(got.per_texture, vec![Some(descs[0])]);
        assert_eq!(got.unplaced, vec![descs[3]]);
        assert_eq!(got.transparent, vec![descs[1], descs[2]]);

        let some_count = got.per_texture.iter().filter(|d| d.is_some()).count();
        assert_eq!(
            some_count + got.unplaced.len() + got.transparent.len(),
            descs.len()
        );
    }

    #[test]
    fn describe_textures_with_no_manifest_is_all_none_and_empty_unplaced() {
        let texs = [sr_tex(1, 64, 64), sr_tex(2, 32, 32)];
        let got = describe_textures(&texs, &[]);
        assert_eq!(got.per_texture, vec![None, None]);
        assert!(got.unplaced.is_empty());
        assert!(got.transparent.is_empty());
    }
}
