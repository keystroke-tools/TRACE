use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use tauri::{
    App, AppHandle, Manager, Window, WindowEvent,
    menu::{Menu, MenuItem},
    plugin::PermissionState,
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};
use tauri_plugin_notification::NotificationExt;

const OPEN_MENU_ID: &str = "trace-tray-open";
const QUIT_MENU_ID: &str = "trace-tray-quit";

#[derive(Clone, Default)]
pub(crate) struct CloseToTrayState {
    enabled: Arc<AtomicBool>,
    notified: Arc<AtomicBool>,
}

impl CloseToTrayState {
    pub(crate) fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    pub(crate) fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }

    fn should_notify(&self) -> bool {
        !self.notified.swap(true, Ordering::Relaxed)
    }
}

pub(crate) fn setup(app: &mut App) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, OPEN_MENU_ID, "Open TRACE", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, QUIT_MENU_ID, "Quit TRACE", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    let mut builder = TrayIconBuilder::with_id("trace-main")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("TRACE")
        .on_menu_event(|app, event| match event.id.as_ref() {
            OPEN_MENU_ID => show_main_window(app),
            QUIT_MENU_ID => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } | TrayIconEvent::DoubleClick {
                    button: MouseButton::Left,
                    ..
                }
            ) {
                show_main_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }
    builder.build(app)?;
    Ok(())
}

pub(crate) fn handle_window_event(window: &Window, event: &WindowEvent) {
    if window.label() != "main" {
        return;
    }
    let WindowEvent::CloseRequested { api, .. } = event else {
        return;
    };
    let state = window.state::<CloseToTrayState>();
    if !state.enabled() {
        return;
    }
    api.prevent_close();
    if window.hide().is_ok() && state.should_notify() {
        notify_running_in_tray(window.app_handle());
    }
}

fn show_main_window(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
}

fn notify_running_in_tray(app: &AppHandle) {
    let notification = app.notification();
    let permission = notification
        .permission_state()
        .unwrap_or(PermissionState::Denied);
    let permission = if matches!(
        permission,
        PermissionState::Prompt | PermissionState::PromptWithRationale
    ) {
        notification
            .request_permission()
            .unwrap_or(PermissionState::Denied)
    } else {
        permission
    };
    if permission == PermissionState::Granted {
        let _ = notification
            .builder()
            .title("TRACE is still running")
            .body("TRACE will keep recording in the background. Use the tray icon to reopen or quit it.")
            .show();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tray_notification_is_only_requested_once_per_run() {
        let state = CloseToTrayState::default();
        assert!(state.should_notify());
        assert!(!state.should_notify());
    }

    #[test]
    fn close_to_tray_is_opt_in() {
        let state = CloseToTrayState::default();
        assert!(!state.enabled());
        state.set_enabled(true);
        assert!(state.enabled());
    }
}
