//! Replayer configuration applied via `MTLREPLAYER_*` env vars before init.

/// The load-relevant replayer knobs.
///
/// Applied by `ReplayerConfig::apply_env`, which [`crate::Session::configure_env`]
/// calls before the framework is bootstrapped.
///
/// Applying a config is authoritative over the exactly three `MTLREPLAYER_*`
/// variables it owns (`FORCE_LOAD_UNUSED_RESOURCE`, `IGNORE_UNUSED_RESOURCE`,
/// `OVERRIDE_DEVICE_REGISTRY_ID`): a `true`/`Some` field sets its var, and a
/// `false`/`None` field clears it, even if something set it earlier (so a
/// stale export cannot change behaviour - in particular, a leftover `IGNORE`
/// export cannot silently override a fresh `force_load_unused_resources`,
/// dossier 00). Any other `MTLREPLAYER_*` variable (e.g.
/// `MTLREPLAYER_LOCK_PARAM_BUFFER_SIZE_TO_MAX`) is never touched.
#[derive(Debug, Clone, Default)]
pub struct ReplayerConfig {
    /// Load resources the captured commands never use (else they are
    /// unfetchable).
    pub force_load_unused_resources: bool,
    /// Tolerate unused-resource creation failures during load. NOTE: this
    /// OVERRIDES `force_load_unused_resources` - when both are set the replayer
    /// honours ignore and skips the unused resources (they stay unfetchable),
    /// so the two are not additive. Set at most one (dossier 00, MEASURED).
    pub ignore_unused_resources: bool,
    /// Bind a specific device by registryID (the only reachable device
    /// lever).
    pub device_registry_id: Option<u64>,
}

impl ReplayerConfig {
    /// Reconcile the three owned env vars against this config: set when the
    /// field is on, clear (`remove_var`) when off/None. Call before
    /// `GTMTLReplayController_init`.
    ///
    /// # Safety
    /// Must be called while the process is single-threaded (before any thread
    /// that reads the environment is spawned), like all `set_var` in this crate.
    pub(crate) unsafe fn apply_env(&self) {
        // SAFETY: caller guarantees single-threaded (Session::configure_env,
        // its only caller).
        unsafe {
            if self.force_load_unused_resources {
                std::env::set_var("MTLREPLAYER_FORCE_LOAD_UNUSED_RESOURCE", "1");
            } else {
                std::env::remove_var("MTLREPLAYER_FORCE_LOAD_UNUSED_RESOURCE");
            }
            if self.ignore_unused_resources {
                std::env::set_var("MTLREPLAYER_IGNORE_UNUSED_RESOURCE", "1");
            } else {
                std::env::remove_var("MTLREPLAYER_IGNORE_UNUSED_RESOURCE");
            }
            if let Some(id) = self.device_registry_id {
                std::env::set_var("MTLREPLAYER_OVERRIDE_DEVICE_REGISTRY_ID", id.to_string());
            } else {
                std::env::remove_var("MTLREPLAYER_OVERRIDE_DEVICE_REGISTRY_ID");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_sets_nothing() {
        // A pure smoke: default has all-off / None.
        let c = ReplayerConfig::default();
        assert!(!c.force_load_unused_resources);
        assert_eq!(c.device_registry_id, None);
    }

    // Single test fn doing the whole set/clear/assert sequence, so it never
    // races another env-touching test over these three vars.
    #[test]
    fn apply_env_is_authoritative_over_its_owned_vars() {
        const FORCE: &str = "MTLREPLAYER_FORCE_LOAD_UNUSED_RESOURCE";
        const IGNORE: &str = "MTLREPLAYER_IGNORE_UNUSED_RESOURCE";
        const REGISTRY: &str = "MTLREPLAYER_OVERRIDE_DEVICE_REGISTRY_ID";

        // SAFETY: test-only env writes; this is the sole test touching these
        // vars, run in one fn so there's no cross-test race.
        unsafe {
            std::env::set_var(FORCE, "1");
            std::env::set_var(IGNORE, "1");
        }

        // Applying an all-off/None config must clear stale exports.
        unsafe { ReplayerConfig::default().apply_env() };
        assert!(std::env::var(FORCE).is_err());
        assert!(std::env::var(IGNORE).is_err());
        assert!(std::env::var(REGISTRY).is_err());

        // force_load_unused_resources: true sets FORCE and (re-)clears IGNORE.
        let cfg = ReplayerConfig {
            force_load_unused_resources: true,
            ..ReplayerConfig::default()
        };
        unsafe { cfg.apply_env() };
        assert_eq!(std::env::var(FORCE).as_deref(), Ok("1"));
        assert!(std::env::var(IGNORE).is_err());

        // Clean up after ourselves regardless of what ran above.
        unsafe { ReplayerConfig::default().apply_env() };
        assert!(std::env::var(FORCE).is_err());
        assert!(std::env::var(IGNORE).is_err());
        assert!(std::env::var(REGISTRY).is_err());
    }
}
