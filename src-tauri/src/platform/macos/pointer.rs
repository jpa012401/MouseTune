//! Pointer freeze during drags (Phase 6 design §2). Humble object.

use objc2_core_graphics::CGAssociateMouseAndMouseCursorPosition;

use crate::input::intents::PointerFreeze;

pub struct MacPointerFreeze;

// Verified by review, not tests: a test calling this would freeze the
// maintainer's real cursor.
impl PointerFreeze for MacPointerFreeze {
    fn set_frozen(&mut self, frozen: bool) {
        // `connected = false` keeps the cursor still while deltas still arrive.
        let _ = CGAssociateMouseAndMouseCursorPosition(!frozen);
    }
}

/// Unconditional unfreeze for paths that can't reach the engine (a caught
/// panic in a callout). Idempotent.
pub fn release() {
    let _ = CGAssociateMouseAndMouseCursorPosition(true);
}
