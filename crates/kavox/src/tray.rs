//! System tray icon & menu: StatusNotifierItem (`ksni`) on Linux, `tray-icon` + `muda` on Windows.

use std::sync::mpsc::Sender;

use kavox_core::Controller;

use crate::state::AppCommand;

pub struct TrayHandle {
    #[cfg(target_os = "linux")]
    inner: Option<ksni::blocking::Handle<LinuxTray>>,
    #[cfg(windows)]
    inner: Option<WindowsTray>,
}

impl TrayHandle {
    pub fn create(
        controller: Controller,
        cmd_tx: Sender<AppCommand>,
        muted: bool,
        voice_enabled: bool,
    ) -> Self {
        #[cfg(target_os = "linux")]
        {
            use ksni::blocking::TrayMethods;
            let icon = crate::platform::decoded_icon().map(|(rgba, w, h)| {
                let mut argb = Vec::with_capacity(rgba.len());
                for px in rgba.as_chunks::<4>().0 {
                    argb.extend_from_slice(&[px[3], px[0], px[1], px[2]]);
                }
                ksni::Icon {
                    width: w as i32,
                    height: h as i32,
                    data: argb,
                }
            });
            let tray = LinuxTray {
                controller,
                cmd_tx,
                muted,
                voice_enabled,
                icon,
            };
            let handle = match tray.spawn() {
                Ok(h) => Some(h),
                Err(e) => {
                    tracing::warn!("no tray icon: {e}");
                    None
                }
            };
            Self { inner: handle }
        }

        #[cfg(windows)]
        {
            match WindowsTray::create(controller, cmd_tx, muted, voice_enabled) {
                Ok(inner) => Self { inner: Some(inner) },
                Err(e) => {
                    tracing::warn!("no tray icon: {e}");
                    Self { inner: None }
                }
            }
        }

        #[cfg(not(any(target_os = "linux", windows)))]
        {
            let _ = (controller, cmd_tx, muted, voice_enabled);
            Self {}
        }
    }

    pub fn set_muted(&self, muted: bool) {
        #[cfg(target_os = "linux")]
        if let Some(h) = &self.inner {
            h.update(|t| t.muted = muted);
        }
        #[cfg(windows)]
        if let Some(t) = &self.inner {
            t.mute.set_checked(muted);
        }
        #[cfg(not(any(target_os = "linux", windows)))]
        let _ = muted;
    }

    pub fn set_voice_enabled(&self, enabled: bool) {
        #[cfg(target_os = "linux")]
        if let Some(h) = &self.inner {
            h.update(|t| t.voice_enabled = enabled);
        }
        #[cfg(windows)]
        if let Some(t) = &self.inner {
            t.voice.set_checked(enabled);
        }
        #[cfg(not(any(target_os = "linux", windows)))]
        let _ = enabled;
    }

    pub fn shutdown(&self) {
        #[cfg(target_os = "linux")]
        if let Some(h) = &self.inner {
            h.shutdown();
        }
    }
}

#[cfg(target_os = "linux")]
struct LinuxTray {
    controller: Controller,
    cmd_tx: Sender<AppCommand>,
    muted: bool,
    voice_enabled: bool,
    icon: Option<ksni::Icon>,
}

#[cfg(target_os = "linux")]
impl ksni::Tray for LinuxTray {
    fn id(&self) -> String {
        crate::platform::APP_ID.into()
    }

    fn title(&self) -> String {
        "Kavox".into()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        self.icon.clone().into_iter().collect()
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = self.cmd_tx.send(AppCommand::ShowWindow);
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::*;
        vec![
            StandardItem {
                label: "Show Kavox".into(),
                activate: Box::new(|this: &mut Self| {
                    let _ = this.cmd_tx.send(AppCommand::ShowWindow);
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Stop all sounds".into(),
                activate: Box::new(|this: &mut Self| {
                    this.controller.lock().stop_all();
                }),
                ..Default::default()
            }
            .into(),
            CheckmarkItem {
                label: "Mute mic".into(),
                checked: self.muted,
                activate: Box::new(|this: &mut Self| {
                    this.controller.lock().set_mic_muted(None);
                }),
                ..Default::default()
            }
            .into(),
            CheckmarkItem {
                label: "Voice changer".into(),
                checked: self.voice_enabled,
                activate: Box::new(|this: &mut Self| {
                    this.controller.lock().set_voice_enabled(None);
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit Kavox".into(),
                activate: Box::new(|this: &mut Self| {
                    let _ = this.cmd_tx.send(AppCommand::Quit);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

#[cfg(windows)]
struct WindowsTray {
    _tray: tray_icon::TrayIcon,
    mute: muda::CheckMenuItem,
    voice: muda::CheckMenuItem,
}

#[cfg(windows)]
impl WindowsTray {
    fn create(
        controller: Controller,
        cmd_tx: Sender<AppCommand>,
        muted: bool,
        voice_enabled: bool,
    ) -> Result<Self, String> {
        use muda::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
        use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

        let show = MenuItem::with_id("show", "Show Kavox", true, None);
        let stop = MenuItem::with_id("stop_all", "Stop all sounds", true, None);
        let mute = CheckMenuItem::with_id("mute", "Mute mic", true, muted, None);
        let voice = CheckMenuItem::with_id("voice", "Voice changer", true, voice_enabled, None);
        let quit = MenuItem::with_id("quit", "Quit Kavox", true, None);
        let menu = Menu::with_items(&[
            &show,
            &stop,
            &mute,
            &voice,
            &PredefinedMenuItem::separator(),
            &quit,
        ])
        .map_err(|e| e.to_string())?;

        let mut builder = TrayIconBuilder::new()
            .with_tooltip("Kavox")
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false);
        if let Some((rgba, w, h)) = crate::platform::decoded_icon()
            && let Ok(icon) = Icon::from_rgba(rgba, w, h)
        {
            builder = builder.with_icon(icon);
        }
        let tray = builder.build().map_err(|e| e.to_string())?;

        let cmd_menu = cmd_tx.clone();
        std::thread::Builder::new()
            .name("kavox-tray-menu".into())
            .spawn(move || {
                let rx = MenuEvent::receiver();
                while let Ok(event) = rx.recv() {
                    match event.id.0.as_str() {
                        "show" => {
                            let _ = cmd_menu.send(AppCommand::ShowWindow);
                        }
                        "stop_all" => controller.lock().stop_all(),
                        "mute" => {
                            controller.lock().set_mic_muted(None);
                        }
                        "voice" => {
                            controller.lock().set_voice_enabled(None);
                        }
                        "quit" => {
                            let _ = cmd_menu.send(AppCommand::Quit);
                        }
                        _ => {}
                    }
                }
            })
            .ok();

        std::thread::Builder::new()
            .name("kavox-tray-click".into())
            .spawn(move || {
                let rx = TrayIconEvent::receiver();
                while let Ok(event) = rx.recv() {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let _ = cmd_tx.send(AppCommand::ShowWindow);
                    }
                }
            })
            .ok();

        Ok(Self {
            _tray: tray,
            mute,
            voice,
        })
    }
}
