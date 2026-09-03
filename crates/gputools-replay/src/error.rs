//! Error types for the safe API.

/// Errors from opening or driving a [`crate::Session`].
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    /// A session is already open in this process (one session per process).
    #[error("a replay session is already open in this process (one per process)")]
    AlreadyOpen,
    /// The `MTLREPLAYER_LOCK_PARAM_BUFFER_SIZE_TO_MAX=0` unlock var is not set.
    #[error("replayer unlock env not set: {0}")]
    UnlockEnv(String),
    /// The capture bundle path is missing a required entry.
    #[error("capture bundle is malformed: {0}")]
    BadBundle(String),
    /// APR bootstrap failed.
    #[error("APR bootstrap failed (code {code})")]
    Apr {
        /// Error code from APR.
        code: i32,
    },
    /// `-load:error:` reported failure, or the error observer recorded one.
    #[error("the replayer reported an error: {message}")]
    Replayer {
        /// Error message from the replayer.
        message: String,
    },
    /// `-load:` returned NO without an observed error.
    #[error("loading the capture failed: {bundle}")]
    LoadFailed {
        /// Path to the capture bundle.
        bundle: String,
    },
}

/// Errors from a fetch.
#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    /// The request slice was empty.
    #[error("fetch batch was empty")]
    EmptyBatch,
    /// `-fetch:` returned no token.
    #[error("fetch returned no token")]
    NoToken,
    /// The completion handler did not fire within the timeout.
    #[error("fetch timed out")]
    Timeout,
    /// No response object was delivered.
    #[error("fetch delivered no response")]
    NoResponse,
    /// The reply carried no data.
    #[error("fetch reply had no data")]
    NoData,
    /// The replayer reported an error attributed to this fetch.
    #[error("the replayer reported an error for the fetch: {message}")]
    Replayer {
        /// Error message from the replayer.
        message: String,
    },
    /// The reply could not be parsed.
    #[error("reply parse failed: {0}")]
    Parse(String),
    /// Building the fetch request/batch itself failed: an ObjC class the
    /// framework should export was not registered, or `alloc`/`init` on one
    /// returned nil. Distinct from [`FetchError::Parse`], which is about a
    /// reply's bytes, not about setting up the request that would fetch
    /// them.
    #[error("fetch setup failed: {0}")]
    Setup(String),
}

/// Errors from parsing a harvester capture block.
#[derive(Debug, thiserror::Error)]
pub enum HarvesterError {
    /// The block does not start with the "capture" magic.
    #[error("not a capture block: wrong magic")]
    BadMagic,
    /// The buffer is too small to hold the claimed metadata/planes.
    #[error("capture block is truncated")]
    Truncated,
    /// The block is not a texture-type block.
    #[error("capture block is not a texture block (type {0})")]
    WrongType(u16),
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn errors_display_their_message() {
        assert_eq!(
            SessionError::AlreadyOpen.to_string(),
            "a replay session is already open in this process (one per process)"
        );
        assert_eq!(
            HarvesterError::BadMagic.to_string(),
            "not a capture block: wrong magic"
        );
    }
}
