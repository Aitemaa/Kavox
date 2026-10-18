use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

use crate::{AppState, lock, show_main_window};

pub struct TrayItems {
    mute: CheckMenuItem<Wry>,
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let state = app.state::<AppState>();
    let muted = state.controller.lock().settings().audio.mic_muted;
    let show = MenuItem::with_id(app, "show", "Show Kavox", true, None::<&str>)?;
    let stop = MenuItem::with_id(app, "stop_all", "Stop all sounds", true, None::<&str>)?;
    let mute = CheckMenuItem::with_id(app, "mute", "Mute mic", true, muted, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Kavox", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &show,
            &stop,
            &mute,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;

    let mut builder = TrayIconBuilder::with_id("main")
        .tooltip("Kavox")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            let state = app.state::<AppState>();
            match event.id().as_ref() {
                "show" => show_main_window(app),
                "stop_all" => state.controller.lock().stop_all(),
                "mute" => {
                    state.controller.lock().set_mic_muted(None);
                }
                "quit" => app.exit(0),
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    *lock(&state.tray) = Some(TrayItems { mute });
    Ok(())
}

/// Called from core events (core locked), so it only posts to the main thread and never waits.
pub fn set_muted(app: &AppHandle, muted: bool) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let Some(mute) = lock(&state.tray).as_ref().map(|t| t.mute.clone()) else {
        return;
    };
    let _ = app.run_on_main_thread(move || {
        let _ = mute.set_checked(muted);
    });
}
