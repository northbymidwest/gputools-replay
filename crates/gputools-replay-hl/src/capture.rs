//! [`Capture`]: the high-level entry point. Owns one substrate [`Session`],
//! and turns each fetch reply into domain handles sharing one aligned
//! buffer (see `bytes.rs`).

use crate::Error;
use crate::accel::AccelStructure;
use crate::buffer::{Buffer, Heap};
use crate::bytes::{AlignedBuf, Payload};
use crate::image::{Texture, Wireframe};
use crate::pipeline::Pipeline;
use gputools_replay::Session;
use gputools_replay::config::ReplayerConfig;
use gputools_replay::reply::Record;
use gputools_replay::request::{DispatchUid, Region, TextureRequest, WireframeRequest};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

/// A high-level capture: owns one substrate session, yields decode-on-demand
/// domain handles. One `Capture` per process (the substrate's one-session
/// guard).
pub struct Capture {
    session: Session,
    timeout: Duration,
    path: std::path::PathBuf,
    // `Err(())` distinguishes "the bundle failed to parse" from "it parsed
    // but describes no textures" (the two `ManifestStatus` failure modes);
    // the parse error itself carries no information callers need.
    bundle: std::cell::OnceCell<Result<gputrace_bundle::Bundle, ()>>,
}

/// The manifest's condition for a [`Capture`], from [`Capture::manifest_status`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManifestStatus {
    /// The manifest parsed and describes this many textures.
    Ok(usize),
    /// The manifest parsed but describes no textures.
    NoDescriptors,
    /// The manifest bundle could not be opened or parsed.
    Unparseable,
}

/// Which aspect of a combined depth/stencil texture to fetch. A combined
/// `Depth32Float_Stencil8` texture is served per-aspect (never as one packed
/// format): the depth aspect via `plane 0`, the stencil aspect via `plane 1`,
/// both on the same streamRef (MEASURED).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Aspect {
    /// The depth aspect (`Depth32Float`), fetched via plane 0.
    Depth,
    /// The stencil aspect (`X32_Stencil8`, one byte per pixel), fetched via
    /// plane 1.
    Stencil,
}

/// Copies one reply's whole `data` blob into a single shared aligned buffer,
/// so every record decoded from that reply can slice it without copying
/// again.
fn shared(data: &[u8]) -> Arc<AlignedBuf> {
    Arc::new(AlignedBuf::from_bytes(data))
}

impl Capture {
    /// Apply `cfg`'s `MTLREPLAYER_*` env vars. Call once, early in `main`,
    /// while the process is single-threaded. Forwards to
    /// `Session::configure_env`.
    ///
    /// # Safety
    /// Writes environment variables; sound only single-threaded (see the
    /// substrate).
    pub unsafe fn configure_env(cfg: &ReplayerConfig) {
        // SAFETY: forwarded precondition - caller upholds single-threadedness.
        unsafe { Session::configure_env(cfg) }
    }

    /// Open a capture bundle. Safe: no env write here.
    pub fn open(path: &Path) -> Result<Self, Error> {
        Ok(Self {
            session: Session::open(path)?,
            timeout: Duration::from_secs(60),
            path: path.to_path_buf(),
            bundle: std::cell::OnceCell::new(),
        })
    }

    /// Open (and cache) the bundle, remembering only whether it parsed.
    fn open_bundle(&self) -> &Result<gputrace_bundle::Bundle, ()> {
        self.bundle
            .get_or_init(|| gputrace_bundle::Bundle::open(&self.path).map_err(|_| ()))
    }

    /// Set the per-fetch timeout (default 60s). Fixture-scale captures
    /// (`fixture-apps/`) fetch in well under a second; the 60s default is
    /// generous headroom for large third-party captures, which can take
    /// anywhere from ~27 seconds to 20+ minutes (docs/HANDOFF.md).
    pub fn set_timeout(&mut self, t: Duration) {
        self.timeout = t;
    }

    /// Advance playback to the end.
    pub fn play_all(&self) {
        self.session.play_all();
    }

    /// Play to a command index.
    pub fn play_to(&self, index: u32) {
        self.session.play_to(index);
    }

    /// Rewind to the start.
    pub fn rewind(&self) {
        self.session.rewind();
    }

    /// The current command index.
    pub fn command_index(&self) -> u32 {
        self.session.command_index()
    }

    /// Fetch textures by streamRef (natural size, level 0, slice 0).
    ///
    /// The fetch can emit a given streamRef more than once in one reply
    /// (MEASURED: corpus commands 1121/1122); the descriptor join dedupes
    /// internally, but a caller keying its own results on stream_ref should
    /// dedupe too.
    pub fn textures(&self, refs: impl IntoIterator<Item = u64>) -> Result<Vec<Texture>, Error> {
        let reqs: Vec<TextureRequest> = refs.into_iter().map(TextureRequest::natural).collect();
        self.textures_with(&reqs)
    }

    /// Fetch a specific aspect of depth/stencil textures by streamRef.
    /// `Aspect::Depth` -> the depth plane (`Depth32Float`), `Aspect::Stencil`
    /// -> the stencil plane (`X32_Stencil8`, one byte per pixel). For a plain
    /// (non-combined) texture, prefer [`Capture::textures`].
    ///
    /// Plane 1 is inert on an ordinary (non-combined) texture - it echoes the
    /// texture's own content rather than any real stencil data. This call
    /// does not know a streamRef's provenance, so `texture_aspects(_,
    /// Aspect::Stencil)` returns whatever plane 1 answers, unfiltered
    /// (faithful to the fetch). Callers wanting only genuine stencil data
    /// should filter with `FormatKind::is_stencil_only()` against the
    /// fetched `Texture`'s plane provenance.
    pub fn texture_aspects(
        &self,
        refs: impl IntoIterator<Item = u64>,
        aspect: Aspect,
    ) -> Result<Vec<Texture>, Error> {
        let plane = match aspect {
            Aspect::Depth => 0,
            Aspect::Stencil => 1,
        };
        let reqs: Vec<TextureRequest> = refs
            .into_iter()
            .map(|r| TextureRequest::new(r, Region::ZERO, plane))
            .collect();
        self.textures_with(&reqs)
    }

    /// Fetch textures with full control (region/plane/slice/level).
    pub fn textures_with(&self, reqs: &[TextureRequest]) -> Result<Vec<Texture>, Error> {
        if reqs.is_empty() {
            return Ok(Vec::new());
        }
        let reply = self.session.fetch_textures(reqs, self.timeout)?;
        let buf = shared(&reply.raw().data);
        Ok(reply
            .records()
            .iter()
            .map(|r| {
                let (o, n) = r.data_range();
                Texture::from_parts(r, Payload::new(buf.clone(), o, n))
            })
            .collect())
    }

    /// Fetch raw buffer contents by streamRef.
    pub fn buffers(&self, refs: impl IntoIterator<Item = u64>) -> Result<Vec<Buffer>, Error> {
        let refs: Vec<u64> = refs.into_iter().collect();
        if refs.is_empty() {
            return Ok(Vec::new());
        }
        let reply = self.session.fetch_buffers(&refs, self.timeout)?;
        let buf = shared(&reply.raw().data);
        Ok(reply
            .records()
            .iter()
            .map(|r| {
                let (o, n) = r.data_range();
                Buffer::from_parts(r.stream_ref as u64, Payload::new(buf.clone(), o, n))
            })
            .collect())
    }

    /// Fetch raw heap contents by streamRef.
    pub fn heaps(&self, refs: impl IntoIterator<Item = u64>) -> Result<Vec<Heap>, Error> {
        let refs: Vec<u64> = refs.into_iter().collect();
        if refs.is_empty() {
            return Ok(Vec::new());
        }
        let reply = self.session.fetch_heaps(&refs, self.timeout)?;
        let buf = shared(&reply.raw().data);
        Ok(reply
            .records()
            .iter()
            .map(|r| {
                let (o, n) = r.data_range();
                Heap::from_parts(r.stream_ref as u64, Payload::new(buf.clone(), o, n))
            })
            .collect())
    }

    /// Fetch raw acceleration-structure contents by streamRef.
    pub fn acceleration_structures(
        &self,
        refs: impl IntoIterator<Item = u64>,
    ) -> Result<Vec<AccelStructure>, Error> {
        let refs: Vec<u64> = refs.into_iter().collect();
        if refs.is_empty() {
            return Ok(Vec::new());
        }
        let reply = self
            .session
            .fetch_acceleration_structures(&refs, self.timeout)?;
        let buf = shared(&reply.raw().data);
        Ok(reply
            .records()
            .iter()
            .map(|r| {
                let (o, n) = r.data_range();
                AccelStructure::from_parts(Payload::new(buf.clone(), o, n))
            })
            .collect())
    }

    /// Fetch pipeline binaries by streamRef.
    pub fn pipeline_binaries(
        &self,
        refs: impl IntoIterator<Item = u64>,
    ) -> Result<Vec<Pipeline>, Error> {
        let refs: Vec<u64> = refs.into_iter().collect();
        if refs.is_empty() {
            return Ok(Vec::new());
        }
        let reply = self.session.fetch_pipeline_binaries(&refs, self.timeout)?;
        let buf = shared(&reply.raw().data);
        Ok(reply
            .records()
            .iter()
            .map(|r| {
                let (o, n) = r.data_range();
                Pipeline::from_parts(r.pipeline_id, r.handle, Payload::new(buf.clone(), o, n))
            })
            .collect())
    }

    /// Fetch rendered wireframe images for `draws` (solid fill vs wireframe
    /// lines controlled by `solid`).
    pub fn wireframes(
        &self,
        draws: impl IntoIterator<Item = u64>,
        solid: bool,
    ) -> Result<Vec<Wireframe>, Error> {
        let reqs: Vec<WireframeRequest> = draws
            .into_iter()
            .map(|d| WireframeRequest {
                dispatch_uid: DispatchUid(d),
                solid,
            })
            .collect();
        self.wireframes_with(&reqs)
    }

    /// Fetch rendered wireframe images with full control over requests.
    pub fn wireframes_with(&self, reqs: &[WireframeRequest]) -> Result<Vec<Wireframe>, Error> {
        if reqs.is_empty() {
            return Ok(Vec::new());
        }
        let reply = self.session.fetch_wireframes(reqs, self.timeout)?;
        let buf = shared(&reply.raw().data);
        Ok(reply
            .records()
            .iter()
            .map(|r| {
                let (o, n) = r.data_range();
                Wireframe::from_parts(r, Payload::new(buf.clone(), o, n))
            })
            .collect())
    }

    /// The bundle manifest for this capture, parsed and cached on first use.
    /// `None` when the bundle cannot be read/parsed (metadata degrades, bytes
    /// do not).
    fn manifest(&self) -> Option<&gputrace_bundle::Bundle> {
        self.open_bundle().as_ref().ok()
    }

    /// The manifest's condition: how many textures it describes, that it
    /// parsed but describes none, or that it failed to parse at all.
    pub fn manifest_status(&self) -> ManifestStatus {
        match self.open_bundle() {
            Err(()) => ManifestStatus::Unparseable,
            Ok(b) if b.texture_count() == 0 => ManifestStatus::NoDescriptors,
            Ok(b) => ManifestStatus::Ok(b.texture_count()),
        }
    }

    /// Join already-fetched `texs` against the cached manifest, by the
    /// creation-order ordinal zip (dossier 00). Pure: no fetch, and no error
    /// on a gap - a manifest descriptor nothing claims lands in
    /// [`crate::describe::Descriptions::unplaced`] instead, and a combined
    /// depth+stencil descriptor (never served directly by the fetch) lands in
    /// [`crate::describe::Descriptions::transparent`] (see [`crate::describe`] for the join's
    /// own known limitation). If the manifest is absent, unparseable, or
    /// empty, `per_texture` is all-`None` and `unplaced`/`transparent` are
    /// both empty.
    pub fn describe(&self, texs: &[Texture]) -> crate::describe::Descriptions {
        let descs: Vec<gputrace_bundle::TextureDescriptor> = self
            .manifest()
            .map(|b| b.textures().to_vec())
            .unwrap_or_default();
        crate::describe::describe_textures(texs, &descs)
    }

    /// Fetch textures for `refs`, then join each to its manifest descriptor
    /// via [`Capture::describe`]. Descriptors are `None` for textures the
    /// manifest does not attribute; never errors on a gap.
    pub fn textures_described(
        &self,
        refs: impl IntoIterator<Item = u64>,
    ) -> Result<Vec<crate::describe::DescribedTexture>, Error> {
        use crate::describe::DescribedTexture;
        let texs = self.textures(refs)?;
        let described = self.describe(&texs);
        Ok(texs
            .into_iter()
            .zip(described.per_texture)
            .map(|(texture, descriptor)| DescribedTexture {
                texture,
                descriptor,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_rejects_a_nonexistent_bundle() {
        assert!(Capture::open(Path::new("/nonexistent.gputrace")).is_err());
    }
}
