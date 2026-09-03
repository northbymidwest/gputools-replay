//! The one environment variable the whole approach depends on.

use std::fmt;

/// MEASURED (HANDOFF 2.1): with `lockParameterBufferSizeToMax = 1`, the
/// default, the replay device's command-queue creation returns nil in an
/// unentitled process and every fetch fails. Found by swizzling the ObjC
/// method and observing the nil.
///
/// This crate verifies the variable and never sets it: `std::env::set_var`
/// is unsafe in edition 2024 and sound only while the process is
/// single-threaded, a precondition only a binary's `main` can guarantee.
pub const UNLOCK_ENV: &str = "MTLREPLAYER_LOCK_PARAM_BUFFER_SIZE_TO_MAX";

/// The variable was not the literal `"0"`. `found` is what was there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnlockEnvError {
    pub found: Option<String>,
}

impl fmt::Display for UnlockEnvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{UNLOCK_ENV} must be set to \"0\" before any replay bootstrap \
             (found {:?}); without it the replayer cannot create its command \
             queue in an unentitled process",
            self.found
        )
    }
}

impl std::error::Error for UnlockEnvError {}

/// Pure check, split from the environment read so it can be tested without
/// mutating the process environment (unsound under a threaded test harness).
/// Only the literal `"0"` passes; nothing merely falsy is waved through.
pub fn check_unlock_env(found: Option<&str>) -> Result<(), UnlockEnvError> {
    if found == Some("0") {
        Ok(())
    } else {
        Err(UnlockEnvError {
            found: found.map(str::to_owned),
        })
    }
}

/// Reads the process environment and applies [`check_unlock_env`].
pub fn unlock_env_ok() -> Result<(), UnlockEnvError> {
    let value = std::env::var_os(UNLOCK_ENV).map(|v| v.to_string_lossy().into_owned());
    check_unlock_env(value.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_literal_zero_is_accepted() {
        assert!(check_unlock_env(Some("0")).is_ok());
    }

    #[test]
    fn an_unset_variable_is_refused_and_named() {
        let err = check_unlock_env(None).unwrap_err();
        assert_eq!(err.found, None);
        assert!(
            err.to_string()
                .contains("MTLREPLAYER_LOCK_PARAM_BUFFER_SIZE_TO_MAX")
        );
    }

    #[test]
    fn the_default_locked_setting_is_refused_and_reported() {
        let err = check_unlock_env(Some("1")).unwrap_err();
        assert_eq!(err.found.as_deref(), Some("1"));
    }

    #[test]
    fn only_the_literal_zero_disables_the_lock() {
        for value in ["00", "0.0", " 0", "false", ""] {
            assert!(
                check_unlock_env(Some(value)).is_err(),
                "{value:?} was accepted"
            );
        }
    }
}
