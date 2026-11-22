pub mod feedback;
pub mod mixer_strip;
pub mod pad_editor;
pub mod pads;
pub mod settings;
pub mod setup_wizard;
pub mod top_bar;
pub mod voice_board;

use std::sync::mpsc::Sender;

use eframe::egui;
use kavox_core::{Controller, Error, GlobalHotkey, NoticeLevel};

use crate::hotkey::{self, HotkeyService};
use crate::state::{AppCommand, UiState};

/// Shared runtime handles passed to view renderers so they can invoke core commands cleanly.
pub struct ActionCtx<'a> {
    pub controller: &'a Controller,
    pub hotkeys: &'a mut HotkeyService,
    pub cmd_tx: &'a Sender<AppCommand>,
    pub egui_ctx: &'a egui::Context,
}

impl<'a> ActionCtx<'a> {
    pub fn pick_and_import(&self, state: &UiState) {
        let board = Some(state.library.active_board.clone());
        crate::state::spawn_pick_and_import(
            self.controller.clone(),
            board,
            self.cmd_tx.clone(),
            self.egui_ctx.clone(),
        );
    }

    pub fn set_sound_hotkey(&mut self, id: &str, hotkey_str: Option<String>) -> Result<(), String> {
        if let Some(h) = &hotkey_str {
            hotkey::validate(h).map_err(|e| format!("{h} can't be used as a hotkey: {e}"))?;
        }
        let previous = self
            .controller
            .lock()
            .library()
            .sound(id)
            .map_err(|e| e.to_string())?
            .hotkey
            .clone();
        self.controller
            .lock()
            .set_sound_hotkey(id, hotkey_str.clone())
            .map_err(|e| e.to_string())?;
        self.apply_or_revert(hotkey_str, || {
            self.controller.lock().set_sound_hotkey(id, previous)
        })
    }

    pub fn set_voice_hotkey(
        &mut self,
        preset: &str,
        hotkey_str: Option<String>,
    ) -> Result<(), String> {
        if let Some(h) = &hotkey_str {
            hotkey::validate(h).map_err(|e| format!("{h} can't be used as a hotkey: {e}"))?;
        }
        let previous = self
            .controller
            .lock()
            .settings()
            .voice
            .hotkeys
            .get(preset)
            .cloned();
        self.controller
            .lock()
            .set_voice_hotkey(preset, hotkey_str.clone())
            .map_err(|e| e.to_string())?;
        self.apply_or_revert(hotkey_str, || {
            self.controller.lock().set_voice_hotkey(preset, previous)
        })
    }

    pub fn set_global_hotkey(
        &mut self,
        which: GlobalHotkey,
        hotkey_str: Option<String>,
    ) -> Result<(), String> {
        if let Some(h) = &hotkey_str {
            hotkey::validate(h).map_err(|e| format!("{h} can't be used as a hotkey: {e}"))?;
        }
        let previous = {
            let core = self.controller.lock();
            match which {
                GlobalHotkey::StopAll => core.settings().hotkeys.stop_all.clone(),
                GlobalHotkey::ToggleMute => core.settings().hotkeys.toggle_mute.clone(),
            }
        };
        self.controller
            .lock()
            .set_global_hotkey(which, hotkey_str.clone())
            .map_err(|e| e.to_string())?;
        self.apply_or_revert(hotkey_str, || {
            self.controller.lock().set_global_hotkey(which, previous)
        })
    }

    fn apply_or_revert(
        &mut self,
        hotkey_str: Option<String>,
        revert: impl FnOnce() -> kavox_core::Result<()>,
    ) -> Result<(), String> {
        let failures = self.hotkeys.sync(self.controller);
        let Some(new) = hotkey_str else {
            return Ok(());
        };
        if failures.iter().any(|(h, _)| h.eq_ignore_ascii_case(&new)) {
            let _ = revert();
            self.hotkeys.sync(self.controller);
            return Err(Error::HotkeyTaken(new).to_string());
        }
        Ok(())
    }

    pub fn run_or_toast<T>(&self, state: &mut UiState, res: Result<T, impl ToString>) -> Option<T> {
        match res {
            Ok(v) => Some(v),
            Err(e) => {
                state.push_toast(NoticeLevel::Error, e.to_string());
                None
            }
        }
    }
}
