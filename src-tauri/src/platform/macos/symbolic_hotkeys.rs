//! CGS symbolic hotkeys (spec §7.2). Undocumented WindowServer functions,
//! resolved with `dlsym` at startup instead of linked, so a macOS that drops
//! them still launches MouseTune and falls back to default shortcuts.
//! Signatures are the widely documented reverse-engineered ones; verified
//! on macOS 27 only (see README acceptance).

use std::ffi::{CStr, c_char, c_void};

use crate::actions::{
    OutputError,
    keys::{KeyBinding, ModifierMask},
    perform::{HotkeyState, SymbolicHotkeys},
};

type CGError = i32;
type GetValue = unsafe extern "C" fn(i32, *mut u16, *mut u16, *mut u32) -> CGError;
/// Returns a C `bool` as a byte: a reverse-engineered API must not hand Rust
/// a `bool` that might be neither 0 nor 1.
type IsEnabled = unsafe extern "C" fn(i32) -> u8;
type SetValue = unsafe extern "C" fn(i32, u16, u16, u32) -> CGError;
type SetEnabled = unsafe extern "C" fn(i32, bool) -> CGError;

/// `RTLD_DEFAULT` on macOS.
const RTLD_DEFAULT: *mut c_void = -2isize as *mut c_void;
/// "No character" for the key-equivalent argument.
const NO_CHAR: u16 = 0xFFFF;

unsafe extern "C" {
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

/// Looks up a C function by name.
///
/// # Safety
///
/// `F` must be an `unsafe extern "C" fn` type matching the C signature of
/// `name`; callers pick one of the type aliases above.
pub(crate) unsafe fn resolve<F: Copy>(name: &CStr) -> Option<F> {
    // A function pointer is pointer-sized on every Apple target; refuse to
    // compile for any `F` that isn't, so the copy below can't truncate.
    const { assert!(size_of::<F>() == size_of::<*mut c_void>()) };
    // SAFETY: dlsym with RTLD_DEFAULT and a NUL-terminated name is always safe to call.
    let ptr = unsafe { dlsym(RTLD_DEFAULT, name.as_ptr()) };
    if ptr.is_null() {
        return None;
    }
    debug_assert_eq!(size_of::<F>(), size_of::<*mut c_void>());
    // SAFETY: the caller guarantees F is a function-pointer type matching the
    // symbol's C signature, the sizes are equal (checked at compile time
    // above), and `ptr` is non-null, so the copied value is a valid fn pointer.
    Some(unsafe { std::mem::transmute_copy(&ptr) })
}

pub struct MacSymbolicHotkeys {
    get: GetValue,
    is_enabled: IsEnabled,
    set: SetValue,
    set_enabled: SetEnabled,
}

impl MacSymbolicHotkeys {
    pub fn load() -> Option<Self> {
        Some(Self {
            // SAFETY: each type alias is the documented reverse-engineered
            // C signature of the symbol it resolves.
            get: unsafe { resolve::<GetValue>(c"CGSGetSymbolicHotKeyValue") }?,
            // SAFETY: as above.
            is_enabled: unsafe { resolve::<IsEnabled>(c"CGSIsSymbolicHotKeyEnabled") }?,
            // SAFETY: as above.
            set: unsafe { resolve::<SetValue>(c"CGSSetSymbolicHotKeyValue") }?,
            // SAFETY: as above.
            set_enabled: unsafe { resolve::<SetEnabled>(c"CGSSetSymbolicHotKeyEnabled") }?,
        })
    }
}

impl SymbolicHotkeys for MacSymbolicHotkeys {
    fn read(&self, id: u16) -> Option<HotkeyState> {
        let (mut ch, mut key, mut mods) = (0u16, 0u16, 0u32);
        // SAFETY: valid out-pointers to locals; `get` has the declared signature.
        let err = unsafe { (self.get)(i32::from(id), &mut ch, &mut key, &mut mods) };
        if err != 0 {
            return None;
        }
        // SAFETY: plain call with the declared signature.
        let enabled = unsafe { (self.is_enabled)(i32::from(id)) } != 0;
        Some(HotkeyState {
            enabled,
            binding: KeyBinding {
                key_code: key,
                modifiers: ModifierMask(u64::from(mods)),
            },
        })
    }

    fn install(&self, id: u16, b: KeyBinding) -> Result<(), OutputError> {
        let mods = u32::try_from(b.modifiers.0)
            .map_err(|_| OutputError::Os("CGSSetSymbolicHotKeyValue"))?;
        // SAFETY: plain calls with the declared signatures.
        if unsafe { (self.set)(i32::from(id), NO_CHAR, b.key_code, mods) } != 0 {
            return Err(OutputError::Os("CGSSetSymbolicHotKeyValue"));
        }
        // SAFETY: as above.
        if unsafe { (self.set_enabled)(i32::from(id), true) } != 0 {
            return Err(OutputError::Os("CGSSetSymbolicHotKeyEnabled"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbol_lookup_fails_cleanly_for_a_name_that_does_not_exist() {
        // SAFETY: the symbol doesn't exist, so no pointer is ever produced;
        // `fn()` is pointer-sized as `resolve` requires.
        assert!(unsafe { resolve::<unsafe extern "C" fn()>(c"MouseTuneNoSuchSymbol") }.is_none());
    }

    /// Read-only: reads every supported hotkey's binding; never sets anything.
    #[test]
    #[ignore = "reads WindowServer state; run by hand"]
    fn reads_every_supported_hotkey() {
        let hotkeys = MacSymbolicHotkeys::load().expect("CGS symbols resolve on this macOS");
        for id in crate::actions::perform::ALL_HOTKEYS {
            let (cgs, _) = crate::actions::perform::hotkey(id);
            println!("{id:?} ({cgs}): {:?}", hotkeys.read(cgs));
        }
    }
}
