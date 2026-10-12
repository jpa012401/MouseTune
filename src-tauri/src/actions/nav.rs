//! Per-app Back/Forward strategy (spec §7.3, Phase 4c design §2). The table
//! comes from config; `mt-exec` reads it through an `ArcSwap` that a config
//! observer refreshes, because the bounded job queue may drop jobs and a
//! settings update must never be lost.

use std::{collections::BTreeMap, sync::Arc};

use arc_swap::ArcSwap;

use crate::config::{
    schema::{Config, NavStrategy},
    store::ConfigObserver,
};

pub type NavOverrides = BTreeMap<String, NavStrategy>;
pub type SharedNavOverrides = Arc<ArcSwap<NavOverrides>>;

pub fn shared_nav_overrides(config: &Config) -> SharedNavOverrides {
    Arc::new(ArcSwap::from_pointee(config.nav_overrides.clone()))
}

/// An app missing from the table gets mouse buttons 4/5.
pub fn nav_strategy(overrides: &NavOverrides, bundle_id: Option<&str>) -> NavStrategy {
    bundle_id
        .and_then(|id| overrides.get(id).copied())
        .unwrap_or(NavStrategy::MouseButtons)
}

/// Keeps the executor's table in step with saved settings.
pub struct NavOverridesBridge(pub SharedNavOverrides);

impl ConfigObserver for NavOverridesBridge {
    fn config_changed(&self, config: &Config) {
        self.0.store(Arc::new(config.nav_overrides.clone()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::schema::{
        Config,
        NavStrategy::{CmdBracket, MouseButtons},
    };

    #[test]
    fn a_listed_app_uses_its_strategy_and_others_use_the_buttons() {
        let defaults = Config::default().nav_overrides;
        assert_eq!(
            nav_strategy(&defaults, Some("com.apple.Safari")),
            CmdBracket
        );
        assert_eq!(
            nav_strategy(&defaults, Some("com.google.Chrome")),
            MouseButtons
        );
        assert_eq!(nav_strategy(&defaults, None), MouseButtons);
    }

    #[test]
    fn saving_new_overrides_updates_what_the_executor_reads() {
        let shared = shared_nav_overrides(&Config::default());
        let mut next = Config::default();
        next.nav_overrides
            .insert("com.apple.dt.Xcode".into(), CmdBracket);
        NavOverridesBridge(Arc::clone(&shared)).config_changed(&next);
        assert_eq!(
            nav_strategy(&shared.load(), Some("com.apple.dt.Xcode")),
            CmdBracket
        );
    }
}
