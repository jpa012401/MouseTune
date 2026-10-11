//! MouseTune — smooth scrolling and button remapping for mice on macOS.
//!
//! Architecture and roadmap: `macos_mouse_utility_technical_specification.md`.

#[cfg(not(target_os = "macos"))]
compile_error!("MouseTune only supports macOS.");

mod ipc;
mod logging;
mod platform;
mod tray;
mod window;

use tauri::{Manager, RunEvent};

/// Single registry of IPC commands and events. Shared by the app and the
/// bindings-export test so `src/bindings.ts` can't drift from the backend.
fn specta_builder() -> tauri_specta::Builder<tauri::Wry> {
    tauri_specta::Builder::<tauri::Wry>::new()
        .commands(tauri_specta::collect_commands![ipc::get_status])
}

#[cfg(any(debug_assertions, test))]
fn export_bindings(builder: &tauri_specta::Builder<tauri::Wry>) {
    builder
        .export(
            specta_typescript::Typescript::default()
                .header("// Regenerate with `cargo test` in src-tauri."),
            concat!(env!("CARGO_MANIFEST_DIR"), "/../src/bindings.ts"),
        )
        .expect("failed to export TypeScript bindings");
}

pub fn run() {
    let builder = specta_builder();

    #[cfg(debug_assertions)]
    export_bindings(&builder);

    let app = tauri::Builder::default()
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            builder.mount_events(app);

            let log_guard = logging::init(app.handle())?;
            app.manage(log_guard);

            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            tray::build(app.handle())?;

            tracing::info!(
                version = app.package_info().version.to_string(),
                os = %platform::os_version::current(),
                accessibility = platform::permissions::is_accessibility_trusted(),
                "MouseTune started"
            );

            window::open_settings(app.handle());
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building MouseTune");

    app.run(|app, event| match event {
        // Closing the settings window must not quit the menu bar agent.
        // An explicit `app.exit(code)` (tray → Quit) carries a code and goes through.
        RunEvent::ExitRequested {
            code: None, api, ..
        } => api.prevent_exit(),
        // Launching the app again from Finder/Spotlight opens Settings.
        RunEvent::Reopen { .. } => window::open_settings(app),
        _ => {}
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn export_bindings() {
        super::export_bindings(&super::specta_builder());
    }
}
