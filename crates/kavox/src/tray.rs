//! System tray icon & menu: StatusNotifierItem (`ksni`) on Linux, `tray-icon` on Windows.
//!
//! Ownership stays on the main UI thread inside `KavoxApp` (Windows `tray_icon`/`muda` items use
//! `Rc` internally and must be mutated on the UI thread). On Linux, `ksni` menu callbacks never
//! acquire the core mutex directly and tray state updates are batched off-thread so `ksni`'s
//! internal state lock can never deadlock with `kavox-core` or block the UI on D-Bus.

use std::sync::mpsc::Sender;

use kavox_core::Controller;

use crate::state::AppCommand;

pub struct TrayHandle {
    last_muted: bool,
    last_voice_enabled: bool,
    #[cfg(target_os = "linux")]
    inner: Option<LinuxTrayWorker>,
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
        let inner = LinuxTrayWorker::start(controller, cmd_tx, muted, voice_enabled);

        #[cfg(windows)]
        let inner = match WindowsTray::create(controller, cmd_tx, muted, voice_enabled) {
            Ok(inner) => Some(inner),
            Err(e) => {
                tracing::warn!("no tray icon: {e}");
                None
            }
        };

        #[cfg(not(any(target_os = "linux", windows)))]
        let _ = (controller, cmd_tx);

        Self {
            last_muted: muted,
            last_voice_enabled: voice_enabled,
            #[cfg(any(target_os = "linux", windows))]
            inner,
        }
    }

    /// Updates tray checkmarks only when `muted` or `voice_enabled` actually changed.
    pub fn sync_state(&mut self, muted: bool, voice_enabled: bool) {
        let mute_changed = muted != self.last_muted;
        let voice_changed = voice_enabled != self.last_voice_enabled;
        if !mute_changed && !voice_changed {
            return;
        }
        self.last_muted = muted;
        self.last_voice_enabled = voice_enabled;

        #[cfg(target_os = "linux")]
        if let Some(worker) = &self.inner {
            let _ = worker.sync_tx.send(TraySync::State {
                muted,
                voice_enabled,
            });
        }

        #[cfg(windows)]
        if let Some(t) = &self.inner {
            if mute_changed {
                t.mute.set_checked(muted);
            }
            if voice_changed {
                t.voice.set_checked(voice_enabled);
            }
        }
    }

    pub fn shutdown(&mut self) {
        #[cfg(target_os = "linux")]
        if let Some(mut worker) = self.inner.take() {
            let _ = worker.sync_tx.send(TraySync::Shutdown);
            if let Some(join) = worker.sync_thread.take() {
                let _ = join.join();
            }
        }

        #[cfg(windows)]
        {
            self.inner = None;
        }
    }
}

#[cfg(target_os = "linux")]
enum TrayAction {
    StopAll,
    ToggleMute,
    ToggleVoice,
}

#[cfg(target_os = "linux")]
enum TraySync {
    State { muted: bool, voice_enabled: bool },
    Shutdown,
}

#[cfg(target_os = "linux")]
struct LinuxTrayWorker {
    sync_tx: Sender<TraySync>,
    sync_thread: Option<std::thread::JoinHandle<()>>,
}

#[cfg(target_os = "linux")]
impl LinuxTrayWorker {
    fn start(
        controller: Controller,
        cmd_tx: Sender<AppCommand>,
        muted: bool,
        voice_enabled: bool,
    ) -> Option<Self> {
        use ksni::blocking::TrayMethods;
        use std::sync::mpsc;

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

        let (action_tx, action_rx) = mpsc::channel::<TrayAction>();
        let tray = LinuxTray {
            action_tx,
            cmd_tx,
            muted,
            voice_enabled,
            icon,
        };
        let handle = match tray.spawn() {
            Ok(h) => h,
            Err(e) => {
                tracing::warn!("no tray icon: {e}");
                return None;
            }
        };

        // Runs controller mutations off ksni's callback thread so ksni's lock is never held while
        // acquiring the core mutex.
        let _ = std::thread::Builder::new()
            .name("kavox-tray-action".into())
            .spawn(move || {
                while let Ok(action) = action_rx.recv() {
                    match action {
                        TrayAction::StopAll => controller.lock().stop_all(),
                        TrayAction::ToggleMute => {
                            controller.lock().set_mic_muted(None);
                        }
                        TrayAction::ToggleVoice => {
                            controller.lock().set_voice_enabled(None);
                        }
                    }
                }
            });

        // Runs blocking D-Bus updates off the UI thread and batches bursts to the latest state.
        let (sync_tx, sync_rx) = mpsc::channel::<TraySync>();
        let sync_thread = std::thread::Builder::new()
            .name("kavox-tray-sync".into())
            .spawn(move || {
                while let Ok(mut msg) = sync_rx.recv() {
                    // Coalesce any queued updates so we only issue one D-Bus round-trip.
                    while let Ok(next) = sync_rx.try_recv() {
                        if !matches!(msg, TraySync::Shutdown) {
                            msg = next;
                        }
                    }
                    match msg {
                        TraySync::State {
                            muted,
                            voice_enabled,
                        } => {
                            handle.update(|t| {
                                t.muted = muted;
                                t.voice_enabled = voice_enabled;
                            });
                        }
                        TraySync::Shutdown => {
                            handle.shutdown().wait();
                            break;
                        }
                    }
                }
            })
            .ok();

        Some(Self {
            sync_tx,
            sync_thread,
        })
    }
}

#[cfg(target_os = "linux")]
struct LinuxTray {
    action_tx: Sender<TrayAction>,
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
                    let _ = this.action_tx.send(TrayAction::StopAll);
                }),
                ..Default::default()
            }
            .into(),
            CheckmarkItem {
                label: "Mute mic".into(),
                checked: self.muted,
                activate: Box::new(|this: &mut Self| {
                    let _ = this.action_tx.send(TrayAction::ToggleMute);
                }),
                ..Default::default()
            }
            .into(),
            CheckmarkItem {
                label: "Voice changer".into(),
                checked: self.voice_enabled,
                activate: Box::new(|this: &mut Self| {
                    let _ = this.action_tx.send(TrayAction::ToggleVoice);
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
    mute: tray_icon::menu::CheckMenuItem,
    voice: tray_icon::menu::CheckMenuItem,
}

#[cfg(windows)]
impl WindowsTray {
    fn create(
        controller: Controller,
        cmd_tx: Sender<AppCommand>,
        muted: bool,
        voice_enabled: bool,
    ) -> Result<Self, String> {
        use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
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
