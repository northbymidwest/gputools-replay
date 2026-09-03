//! Guards in front of the replayer. Policy, so it lives here and not in the
//! sys crate.

use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum GuardError {
    /// load: SIGSEGVs on a missing path rather than returning an error.
    #[error("no capture bundle at {0}")]
    MissingBundle(String),
    /// load: SIGSEGVs on a non-capture directory too (measured on an empty
    /// directory: SIGSEGV inside GPUToolsReplay). Inferred requirement from
    /// the captures on hand, not a documented format (HANDOFF 3).
    #[error("{path} is not a capture bundle: it has no {missing} file")]
    NotACaptureBundle { path: String, missing: String },
}

/// Entries every known .gputrace bundle carries.
const REQUIRED_ENTRIES: [&str; 2] = ["index", "metadata"];

/// Rejects anything load: would crash on. Run before any global state is
/// touched, so a rejected path leaves the process able to open a session.
pub fn check_bundle_shape(bundle: &Path) -> Result<(), GuardError> {
    if !bundle.exists() {
        return Err(GuardError::MissingBundle(bundle.display().to_string()));
    }
    for entry in REQUIRED_ENTRIES {
        if !bundle.join(entry).is_file() {
            return Err(GuardError::NotACaptureBundle {
                path: bundle.display().to_string(),
                missing: entry.to_owned(),
            });
        }
    }
    Ok(())
}

/// Sets the unlock variable. Call as the FIRST statement of a probe's `main`
/// and nowhere else.
///
/// # Safety
///
/// `std::env::set_var` is sound only while no other thread can read the
/// environment. At the first statement of `main` the process is
/// single-threaded, which is the one place that precondition genuinely holds
/// (HANDOFF 2.1). The library layers only ever verify.
pub unsafe fn set_unlock_env() {
    unsafe { std::env::set_var(gputools_replay_sys::env::UNLOCK_ENV, "0") };
    gputools_replay_sys::env::unlock_env_ok().expect("just set the unlock env");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    struct TempDir(std::path::PathBuf);
    impl TempDir {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("probes-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
        fn file(&self, name: &str) -> &Self {
            std::fs::write(self.0.join(name), b"x").unwrap();
            self
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_missing_path_is_named_as_missing() {
        let err = check_bundle_shape(Path::new("/no/such/capture.gputrace")).unwrap_err();
        assert!(err.to_string().contains("/no/such/capture.gputrace"));
    }

    /// The important one: an existing non-capture is what SIGSEGVs the
    /// replayer (HANDOFF 3), so it must be rejected before load: sees it.
    #[test]
    fn an_existing_non_capture_is_rejected_by_the_file_it_lacks() {
        let dir = TempDir::new("empty");
        let err = check_bundle_shape(&dir.0).unwrap_err();
        assert!(err.to_string().contains("it has no index file"), "{err}");
    }

    #[test]
    fn a_partial_bundle_names_the_missing_file() {
        let dir = TempDir::new("partial");
        dir.file("index");
        let err = check_bundle_shape(&dir.0).unwrap_err();
        assert!(err.to_string().contains("it has no metadata file"), "{err}");
    }

    #[test]
    fn a_directory_does_not_satisfy_a_required_entry() {
        let dir = TempDir::new("direntry");
        dir.file("metadata");
        std::fs::create_dir(dir.0.join("index")).unwrap();
        assert!(check_bundle_shape(&dir.0).is_err());
    }

    #[test]
    fn a_bundle_with_every_required_entry_passes() {
        let dir = TempDir::new("whole");
        dir.file("index").file("metadata");
        assert!(check_bundle_shape(&dir.0).is_ok());
    }
}
