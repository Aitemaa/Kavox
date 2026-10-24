# Kavox Soundboard: Product Requirements

| | |
|---|---|
| **Feature** | Soundboard with virtual microphone |
| **Version target** | v0.1 (MVP), v0.2 (follow-up) |
| **Platforms** | Windows 10/11 (primary), Linux with PipeWire or PulseAudio (secondary) |
| **Status** | Draft |
| **Tech spec** | [TECH.md](TECH.md) |

---

## 1. Summary

Kavox is a free desktop app that plays sound clips into your microphone, so people in Discord, games, OBS or any voice app hear them alongside your voice. You set up a grid of sounds, bind hotkeys, and press them mid-call.

It covers what people use paid soundboard apps for, without a paywall, account or telemetry.

## 2. Problem

- Popular soundboard apps limit their free tier or are paid outright.
- Free options are scattered. On Windows, users stitch together VB-CABLE, VoiceMeeter and a media player, which is fiddly and easy to break (for example, all system audio ending up in the mic).
- On Linux, Soundux was the main option and is no longer maintained.

Users want one app that "just works": add sounds, press a key, friends hear it.

## 3. Goals

1. Play any local audio clip into a virtual mic with a single click or global hotkey.
2. Mix the user's real mic into the same virtual mic, so voice and sounds go out together.
3. Let the user hear the sounds themselves (monitor) at an independent volume.
4. Make first-time setup on Windows take under 5 minutes, including the virtual cable install.
5. Run in the background with low CPU and memory use, even during games.
6. Same app and same library format on Windows and Linux.

## 4. Non-goals (v0.x)

- Voice effects other than pitch (robot, reverb, echo) and AI voice conversion. These come in a later feature.
- macOS and mobile.
- An online sound library, marketplace or account system.
- Shipping our own Windows audio driver. We depend on an existing virtual cable for now (see §10).
- Telemetry or analytics of any kind.
- Replacing VoiceMeeter or a full audio router.

## 5. Users

| Persona | Context | What they need |
|---|---|---|
| **Gamer on Discord** (primary) | Windows, plays with friends in voice chat, often in fullscreen games | Hotkeys that work in-game, sounds that reach Discord reliably, no setup headaches |
| **Streamer** | Windows, OBS + Discord | Sounds in both the stream and the call, separate monitor volume, stable over long sessions |
| **Linux user** | PipeWire on KDE/GNOME/Hyprland | A maintained soundboard that creates its own virtual mic and works on Wayland |

## 6. Key user journeys

### 6.1 First run on Windows

1. User installs Kavox and opens it.
2. Setup wizard checks for a virtual cable (VB-CABLE or VoiceMeeter).
   - Not found: the wizard explains why it's needed, links to the official download, and waits. After install, it rechecks (a reboot may be needed).
   - Found: it pre-selects "CABLE Input" as Kavox's output.
3. Wizard warns if the virtual cable became the Windows default playback device (a common side effect of installing it) and offers to open Sound settings.
4. User picks their real mic and their headphones.
5. Wizard plays a test sound and shows a level meter moving on the virtual mic.
6. Wizard shows how to set Discord's input device to **CABLE Output**, and recommends turning off Discord noise suppression (it filters out non-voice sounds).

### 6.2 Daily use

1. Kavox starts minimized to the tray (optional autostart).
2. In a call, the user presses a hotkey. Friends hear the sound; the user hears it through the monitor.
3. User presses "Stop all" hotkey to cut every sound.

### 6.3 Building a board

1. User drags audio files onto the window. Each becomes a pad.
2. User renames a pad, sets its color, volume and hotkey.
3. User reorders pads by dragging.

### 6.4 First run on Linux

1. On launch, Kavox creates a virtual mic named **Kavox Mic**.
2. User selects "Kavox Mic" as input in Discord.
3. On Wayland where global hotkeys aren't available, Settings shows how to bind `kavoxctl play <sound>` in the compositor (for example Hyprland `bind = , F13, exec, kavoxctl play airhorn`).
4. When Kavox quits, the virtual mic is removed.

## 7. Functional requirements

Priority: **P0** = required for v0.1, **P1** = v0.2, **P2** = later.

### 7.1 Library and pads

| ID | Pri | Requirement | Acceptance criteria |
|---|---|---|---|
| LIB-1 | P0 | Import sounds via file picker and drag-and-drop | WAV, MP3, OGG Vorbis, FLAC import; unsupported files show a clear error and are skipped |
| LIB-2 | P0 | Imported files are copied into Kavox's library folder | Moving or deleting the original file doesn't break the pad |
| LIB-3 | P0 | Pads show in a grid with name and color | Grid reflows from 1 column (narrow window) to many |
| LIB-4 | P0 | Edit pad: name, color, volume (0–200%), hotkey, retrigger mode | Changes save immediately and survive restart |
| LIB-5 | P0 | Delete pad | File removed from library folder; hotkey unregistered |
| LIB-6 | P0 | Reorder pads by drag | Order persists |
| LIB-7 | P0 | Search/filter pads by name | Filter updates as user types |
| LIB-8 | P1 | Loudness normalization on import | Pads play at a similar perceived volume by default; user volume applies on top |
| LIB-9 | P1 | Multiple boards (tabs) | User can create, rename, delete and switch boards |
| LIB-10 | P1 | Export / import a board as a `.kavox` zip | Round-trip on another machine restores sounds and settings except hotkeys conflicts, which are reported |
| LIB-11 | P2 | Trim start/end of a clip | Trim is non-destructive |
| LIB-12 | P2 | AAC/M4A support | M4A files import |

### 7.2 Playback

| ID | Pri | Requirement | Acceptance criteria |
|---|---|---|---|
| PLAY-1 | P0 | Click a pad to play | Sound reaches the virtual mic within 50 ms of the click (see NFR-1) |
| PLAY-2 | P0 | Several sounds can play at once | At least 16 simultaneous voices without clipping or dropouts |
| PLAY-3 | P0 | Retrigger mode per pad: **Restart**, **Overlap**, **Toggle** | Restart: stops and starts again. Overlap: starts another copy. Toggle: second press stops it |
| PLAY-4 | P0 | Stop a single sound, and Stop all | Stop fades out over ~10 ms (no click) |
| PLAY-5 | P0 | Pads show a playing state and progress | Indicator updates while playing, clears when done |
| PLAY-6 | P0 | Output limiter | Mixing loud sounds never produces hard digital clipping |
| PLAY-7 | P1 | Loop mode per pad | Loops until stopped |

### 7.3 Audio routing

| ID | Pri | Requirement | Acceptance criteria |
|---|---|---|---|
| AUD-1 | P0 | Output sounds to a selected virtual device | Windows: any render device, auto-suggest VB-CABLE/VoiceMeeter. Linux: Kavox's own virtual mic |
| AUD-2 | P0 | Mic passthrough into the virtual device | User's voice is audible in Discord through the virtual mic; can be turned off |
| AUD-3 | P0 | Monitor output to a selected device (headphones) | User hears sounds; mic is **not** monitored by default |
| AUD-4 | P0 | Independent volumes: mic gain, sounds-to-mic, sounds-to-monitor | Each slider affects only its path |
| AUD-5 | P0 | Mic mute toggle (with hotkey) | Muting mic still lets sounds through |
| AUD-6 | P0 | Level meters for mic and virtual output | Meters update at ~30 fps |
| AUD-7 | P0 | Device unplug/replug recovery | Unplugging the headset doesn't crash; Kavox reconnects when it returns or falls back to default and notifies |
| AUD-8 | P1 | Push-to-talk assist (Windows) | While a sound plays, Kavox holds the user's Discord PTT key so the sound is transmitted |
| AUD-9 | P2 | Mic noise gate | Optional gate with threshold slider |
| AUD-10 | P1 | Voice changer (pitch only) | A Voice view lists preset voices (Chipmunk, Deep, Wobbly and others) plus a pitch dial of ±12 semitones; the changed mic goes to the virtual device. Switching voice glides instead of jumping, and turning it on or off doesn't click |
| AUD-11 | P1 | "Hear myself" | Optional, off by default: plays the changed voice in the monitor output. Respects mic mute |

### 7.4 Hotkeys and control

| ID | Pri | Requirement | Acceptance criteria |
|---|---|---|---|
| KEY-1 | P0 | Global hotkey per pad | Works while a fullscreen game has focus (Windows) |
| KEY-2 | P0 | Global hotkeys: Stop all, Mute mic | Configurable in Settings |
| KEY-3 | P0 | Hotkey recorder UI | Press keys to record; Escape cancels; Backspace clears |
| KEY-4 | P0 | Conflict detection | Same combo on two pads, or a combo the OS refuses, shows an error |
| KEY-5 | P0 | Warn on risky bindings | Binding a plain letter/number key without modifiers shows a warning (it would be swallowed system-wide) |
| KEY-6 | P0 | CLI control: `kavoxctl play <name or id>`, `kavoxctl stop`, `kavoxctl stop-all`, `kavoxctl mute` | Commands reach the running instance within 10 ms; used for Wayland compositor bindings and scripting |
| KEY-7 | P1 | Wayland global shortcuts via the XDG GlobalShortcuts portal | Where the desktop supports it, hotkeys work without compositor config |
| KEY-8 | P1 | Global hotkey per voice | Pressing it selects that voice and turns the changer on; pressing it again turns the changer off. Conflicts with pad and global hotkeys are rejected |

### 7.5 App shell

| ID | Pri | Requirement | Acceptance criteria |
|---|---|---|---|
| APP-1 | P0 | System tray icon with Show, Stop all, Mute mic, Quit | Closing the window minimizes to tray (configurable) |
| APP-2 | P0 | Single instance | Launching again focuses the existing window |
| APP-3 | P0 | First-run setup wizard (Windows and Linux variants) | Covers the journeys in §6.1 and §6.4 |
| APP-4 | P0 | Settings persist across restarts | Devices, volumes, hotkeys and window state restore |
| APP-5 | P0 | Light and dark theme following the OS | Matches system setting |
| APP-6 | P1 | Launch on login | Toggle in Settings |
| APP-7 | P1 | In-app update check | Checks GitHub releases; never auto-installs without consent |
| APP-8 | P0 | Log file and "Copy diagnostics" button | Diagnostics include versions, devices and recent errors, nothing personal |

## 8. Non-functional requirements

| ID | Requirement | Target |
|---|---|---|
| NFR-1 | Trigger latency (hotkey press → audio leaves Kavox) | < 50 ms, p95 |
| NFR-2 | Mic passthrough added latency | < 40 ms, plus about 12 ms on average (25 ms at most) while the voice changer is on |
| NFR-3 | CPU while idle in tray | < 1% of one core |
| NFR-4 | CPU while playing 4 sounds + mic | < 5% of one core on a 4-year-old laptop |
| NFR-5 | Memory with 50 pads loaded | < 200 MB total |
| NFR-6 | Installer size | Windows < 15 MB, Linux AppImage < 30 MB |
| NFR-7 | Stability | 8 h session with mic passthrough: no dropouts from clock drift, no memory growth |
| NFR-8 | Startup to tray | < 2 s |
| NFR-9 | Privacy | No network calls except the opt-in update check |
| NFR-10 | Accessibility | Full keyboard navigation of the grid and settings; labels on all controls |

## 9. UX

### 9.1 Main window

```
┌──────────────────────────────────────────────────────────────┐
│ ▐█▌ Kavox      [Board: Main ▾]   🔍 Search…        ⚙  ─ □ ✕ │
├──────────────────────────────────────────────────────────────┤
│ ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐          │
│ │ Airhorn  │ │ Bruh     │ │ Vine boom│ │ Applause │          │
│ │ Ctrl+1   │ │ Ctrl+2 ▶ │ │ Ctrl+3   │ │ —        │          │
│ │          │ │ ▓▓▓▓░░░░ │ │          │ │          │          │
│ └──────────┘ └──────────┘ └──────────┘ └──────────┘          │
│ ┌──────────┐ ┌──────────┐ ┌ ─ ─ ─ ─ ┐                        │
│ │ Sad trom │ │ Ding     │   + Add                            │
│ │ Ctrl+4   │ │ Ctrl+5   │ │ or drop ││                        │
│ └──────────┘ └──────────┘ └ ─ ─ ─ ─ ┘                        │
├──────────────────────────────────────────────────────────────┤
│ 🎙 Mic ▮▮▮▮▯▯▯ [🔇]  Mic gain ━━━●━━  To mic ━━━━●━  Me ━━●━━━ │
│ [■ Stop all  Ctrl+0]                     → CABLE Input  ● OK │
└──────────────────────────────────────────────────────────────┘
```

- Right-click (or long-press) a pad opens the pad editor drawer.
- A **Sounds | Voice** switch in the top bar swaps the pad board for the Voice view: a tile per voice, the pitch dial, the Hear myself switch and a hotkeys section.
- The bottom bar is always visible: mic meter and mute, the three volume sliders, Stop all, and the current output device with a status dot (green OK, yellow fallback, red no virtual device).

### 9.2 Pad editor (drawer)

Name, color swatches, volume slider with a preview button, hotkey recorder, retrigger mode (segmented control), loop toggle (P1), delete.

### 9.3 Settings

- **Audio:** microphone, virtual output (with "Detect" button), monitor output, mic passthrough on/off.
- **Hotkeys:** Stop all, Mute mic; on Linux Wayland, the CLI binding guide.
- **App:** start minimized, close to tray, launch on login (P1), theme, check for updates (P1).
- **About:** version, licenses, "Copy diagnostics", link to logs folder.

### 9.4 Empty and error states

- No sounds: big drop zone with "Drop audio files here or click to add".
- No virtual device (Windows): red banner "Kavox can't reach your voice apps yet" with a button to reopen the setup wizard.
- Device lost: yellow banner naming the device and what Kavox fell back to.

## 10. Dependencies and licensing

| Dependency | Platform | Notes |
|---|---|---|
| **VB-CABLE** (VB-Audio) | Windows | Free (donationware). Its license doesn't allow bundling it in our installer without an agreement, so Kavox links to the official download page and detects it. Confirm current terms before release. |
| VoiceMeeter (VB-Audio) | Windows | Supported as an alternative if already installed |
| PipeWire (with pipewire-pulse) or PulseAudio | Linux | Kavox creates and removes its own virtual mic |
| WebView2 runtime | Windows | Ships with Windows 11; the installer bootstraps it on Windows 10 if missing |

Kavox's own license is still open (see §14).

## 11. Risks and mitigations

| Risk | Impact | Mitigation |
|---|---|---|
| Users can't or won't install VB-CABLE | No sound reaches voice apps on Windows | Clear wizard with screenshots; support VoiceMeeter; evaluate an open-source signed virtual audio driver later (see TECH.md §14) |
| VB-CABLE becomes the default playback device after install | All PC audio goes into the mic | Wizard detects and warns; status bar shows a warning while it's the case |
| Discord noise suppression (Krisp) filters out sounds | Friends hear nothing or choppy sounds | Wizard and docs tell users to set noise suppression to None for the Kavox input |
| Discord voice activity threshold cuts quiet sounds | Sounds start late or cut off | Docs recommend a lower threshold; PTT assist (AUD-8) for PTT users |
| Global hotkeys swallow keys system-wide | Binding "F" breaks typing | Warn on modifier-less bindings (KEY-5); suggest F13–F24 or numpad |
| Wayland has no general global hotkey API | Hotkeys don't work on some Linux desktops | CLI control for compositor bindings (KEY-6); portal support (KEY-7) |
| Anti-cheat dislikes synthetic key input | PTT assist could flag accounts | PTT assist off by default with a warning; uses standard `SendInput`, no hooks or injection |
| Unsigned installer triggers SmartScreen | Users distrust the app | Apply for free open-source code signing (SignPath Foundation) |
| Clock drift between mic and virtual output | Crackles or growing delay over long sessions | Drift compensation in the engine (TECH.md §6.6) |

## 12. Release scope

### v0.1 (MVP)

All **P0** requirements. Windows installer + Linux AppImage. Setup wizard. English UI.

### v0.2

LIB-8 normalization, LIB-9 boards, LIB-10 export/import, PLAY-7 loop, AUD-8 PTT assist, KEY-7 Wayland portal, APP-6 autostart, APP-7 update check, AUR package.

### Later

Trim, noise gate, M4A, localization (Bahasa Indonesia first), then more voice effects (robot, reverb, echo).

## 13. Success metrics

Kavox has no telemetry, so success is measured by tests and public signals:

- **Performance:** NFR-1 to NFR-8 met in the benchmark suite and manual test matrix before each release.
- **Setup:** 5 first-time testers on Windows complete setup unaided in under 5 minutes.
- **Reliability:** No open P0 bugs at release; no "sound didn't reach Discord" reports without a known cause.
- **Adoption (signal only):** GitHub release downloads and stars.

## 14. Open questions

1. **License:** MIT (simplest, most permissive) or GPL-3.0 (keeps forks open)?
2. **Library storage:** copy files into the library (current plan) or reference them in place to save disk space?
3. **Sound packs:** ship a few CC0 starter sounds so the board isn't empty on first run?
4. **Own Windows driver:** worth pursuing later to remove the VB-CABLE step, given the code-signing cost and maintenance?
5. **Name collisions:** check that "Kavox" isn't trademarked or taken by a similar app before the first public release.
