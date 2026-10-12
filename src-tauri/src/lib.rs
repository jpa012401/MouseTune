//! MouseTune — smooth scrolling and button remapping for mice on macOS.
//!
//! Architecture and roadmap: `macos_mouse_utility_technical_specification.md`.

#[cfg(not(target_os = "macos"))]
compile_error!("MouseTune only supports macOS.");

mod accessibility;
// Wired up in Phase 4a Task 8.
#[allow(dead_code)]
mod actions;
mod config;
mod events;
mod input;
mod ipc;
mod logging;
mod permission_monitor;
mod platform;
#[cfg(test)]
mod test_support;
mod tray;
mod window;

use std::sync::{Arc, Mutex};

use accessibility::{Accessibility, SystemAccessibility};
use permission_monitor::PermissionMonitor;
use tauri::{Manager, RunEvent};

/// Single registry of IPC commands and events. Shared by the app and the
/// bindings-export test so `src/bindings.ts` can't drift from the backend.
fn specta_builder() -> tauri_specta::Builder<tauri::Wry> {
    tauri_specta::Builder::<tauri::Wry>::new()
        .commands(tauri_specta::collect_commands![
            ipc::get_status,
            ipc::request_accessibility,
            ipc::reset_accessibility,
            ipc::get_config,
            ipc::set_config,
            ipc::reset_config,
            ipc::dismiss_config_recovery,
        ])
        .events(tauri_specta::collect_events![
            events::PermissionChanged,
            events::InputStatusChanged,
            events::ConfigChanged,
        ])
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

            let accessibility: Arc<dyn Accessibility> = Arc::new(SystemAccessibility);
            app.manage(Arc::clone(&accessibility));

            let config_path = app.path().app_config_dir()?.join("config.json");
            let mut store = config::store::ConfigStore::load(Box::new(
                config::system_file::SystemConfigFile::new(config_path),
            ));
            let general = store.current().general.clone();

            // Read once so the engine and the tray start from the same value.
            // PermissionMonitor reads trust itself and converges on its next poll.
            let trusted = accessibility.is_trusted();
            let input = input::thread::spawn(
                trusted,
                config::schema::tap_demand(store.current()),
                Box::new(events::EmitInputStatus(app.handle().clone())),
            );
            app.manage(Arc::clone(&input));

            tray::build(app.handle(), trusted, &general)?;
            store.subscribe(Box::new(input::thread::InputConfigBridge(Arc::clone(
                &input,
            ))));
            store.subscribe(Box::new(tray::TrayConfigStatus(app.handle().clone())));
            store.subscribe(Box::new(events::EmitConfigChanged(app.handle().clone())));
            app.manage(Mutex::new(store));

            PermissionMonitor::new(
                Arc::clone(&accessibility),
                vec![
                    Box::new(tray::TrayPermissionStatus(app.handle().clone())),
                    Box::new(events::EmitPermissionChanged(app.handle().clone())),
                    Box::new(input::thread::PermissionBridge(Arc::clone(&input))),
                ],
            )
            .spawn();

            tracing::info!(
                version = app.package_info().version.to_string(),
                os = %platform::os_version::current(),
                accessibility = trusted,
                buttons_enabled = general.buttons_enabled,
                scroll_enabled = general.scroll_enabled,
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
        // Tear the event taps down before the process goes.
        RunEvent::Exit => {
            if let Some(input) = app.try_state::<Arc<input::thread::InputHandle>>() {
                input.shutdown();
            }
        }
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
