//! Breaking-change upgrades (design §1). Additive changes need no step.

use serde_json::Value;

use crate::config::schema::CURRENT_VERSION;

/// `STEPS[i]` upgrades a file from version `i + 1` to `i + 2`.
const STEPS: &[fn(Value) -> Value] = &[];

const _: () = assert!(STEPS.len() as u32 + 1 == CURRENT_VERSION);

/// Upgrades `value` from version `from` (1..=CURRENT_VERSION) to CURRENT_VERSION.
pub fn migrate(mut value: Value, from: u32) -> Value {
    for step in STEPS.iter().skip(from.saturating_sub(1) as usize) {
        value = step(value);
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_current_file_passes_through_unchanged() {
        let value = serde_json::json!({ "version": 1, "general": { "scroll_enabled": false } });
        assert_eq!(migrate(value.clone(), 1), value);
    }
}
