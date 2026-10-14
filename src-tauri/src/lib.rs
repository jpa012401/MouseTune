//! MouseTune — smooth scrolling and button remapping for mice on macOS.
//!
//! Architecture and roadmap: `macos_mouse_utility_technical_specification.md`.

#[cfg(not(target_os = "macos"))]
compile_error!("MouseTune only supports macOS.");

mod accessibility;
mod actions;
mod config;
mod events;
mod input;
mod ipc;
mod logging;
mod login_item;
mod opener;
mod permission_monitor;
mod platform;
#[cfg(test)]
mod test_support;
mod tray;
mod window;

use std::{
    sync::{Arc, Mutex, PoisonError},
    thread::JoinHandle,
};

use accessibility::{Accessibility, SystemAccessibility};
use permission_monitor::PermissionMonitor;
use tauri::{Manager, RunEvent};

/// The `mt-exec` thread, joined on exit after `mt-input` has stopped.
struct ExecThread(Mutex<Option<JoinHandle<()>>>);

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
            ipc::start_capture,
            ipc::stop_capture,
            ipc::start_key_capture,
            ipc::stop_key_capture,
            ipc::scroll_curve_preview,
            ipc::get_launch_at_login,
            ipc::set_launch_at_login,
            ipc::open_logs_folder,
            ipc::open_about_link,
        ])
        .events(tauri_specta::collect_events![
            events::PermissionChanged,
            events::InputStatusChanged,
            events::ConfigChanged,
            events::ButtonCaptured,
            events::KeyCaptured,
            events::KeyCaptureCancelled,
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
            let bundle_id = app.config().identifier.clone();
            let agent_path = app
                .path()
                .home_dir()?
                .join("Library/LaunchAgents")
                .join(format!("{bundle_id}.plist"));
            let program = std::env::current_exe()
                .expect("the running executable has a path, needed for the launch agent");
            let agent = Arc::new(login_item::LaunchAgentLoginItem::new(
                Box::new(platform::login_item::MacAgentFile::new(agent_path)),
                &bundle_id,
                &program,
            ));
            // SMAppService exists from macOS 13 (spec §3.4).
            let modern = platform::os_version::current().at_least(13, 0).then(|| {
                Arc::new(platform::login_item::MacAppService) as Arc<dyn login_item::LoginItem>
            });
            app.manage(login_item::startup(modern, agent));
            app.manage(Arc::new(platform::opener::MacOpener) as Arc<dyn opener::Opener>);

            let config_path = app.path().app_config_dir()?.join("config.json");
            let mut store = config::store::ConfigStore::load(Box::new(
                config::system_file::SystemConfigFile::new(config_path),
            ));
            let config = store.current().clone();
            let remap_count = store.current().buttons.remaps.len();

            // Read once so the engine and the tray start from the same value.
            // PermissionMonitor reads trust itself and converges on its next poll.
            let trusted = accessibility.is_trusted();
            let (sink, jobs) = actions::queue::queue();
            let support = platform::output_support();
            let input = input::thread::spawn(
                trusted,
                config::schema::tap_demand(store.current()),
                input::remaps::compile_with(store.current(), support),
                config::schema::engine_scroll(store.current()),
                Box::new(sink),
                Box::new(events::EmitInputStatus(app.handle().clone())),
            );
            app.manage(Arc::clone(&input));
            let nav = actions::nav::shared_nav_overrides(store.current());
            let exec = actions::executor::spawn(
                jobs,
                actions::perform::Outputs {
                    hotkeys: platform::symbolic_hotkeys::MacSymbolicHotkeys::load()
                        .map(|h| Box::new(h) as Box<dyn actions::perform::SymbolicHotkeys>),
                    poster: Box::new(platform::event_poster::MacEventPoster),
                    frontmost: Box::new(platform::frontmost_app::MacFrontmostApp),
                    notices: Box::new(events::EmitNotices(app.handle().clone())),
                    nav: Arc::clone(&nav),
                },
                Arc::clone(&input.shared().actions),
            );
            app.manage(ExecThread(Mutex::new(Some(exec))));

            tray::build(app.handle(), trusted, &config)?;
            store.subscribe(Box::new(input::thread::InputConfigBridge {
                handle: Arc::clone(&input),
                support,
            }));
            store.subscribe(Box::new(actions::nav::NavOverridesBridge(nav)));
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
                buttons_enabled = config.general.buttons_enabled,
                scroll_enabled = config.general.scroll_enabled,
                remaps = remap_count,
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
            // Only after `mt-input` stopped: its engine owned the queue's only
            // sender, so `mt-exec` now drains what's left and returns.
            if let Some(exec) = app.try_state::<ExecThread>() {
                let join = exec.0.lock().unwrap_or_else(PoisonError::into_inner).take();
                if let Some(join) = join
                    && join.join().is_err()
                {
                    tracing::error!("action thread panicked");
                }
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
