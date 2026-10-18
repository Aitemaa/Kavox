import { useState } from 'react';
import { openUrl } from '@tauri-apps/plugin-opener';
import { api, REPO_URL, VB_CABLE_URL, type Theme } from '../api';
import { micLabel, run, toast, updateApp, updateAudio, useStore, type SettingsTab } from '../store';
import { DeviceSelect, HotkeyRecorder, Icon, LogoMark, Modal, Segmented, Switch } from './ui';

const TABS: { id: SettingsTab; label: string }[] = [
  { id: 'audio', label: 'Audio' },
  { id: 'hotkeys', label: 'Hotkeys' },
  { id: 'app', label: 'App' },
  { id: 'about', label: 'About' },
];

export function SettingsDialog() {
  const open = useStore((s) => s.dialog === 'settings');
  const tab = useStore((s) => s.settingsTab);
  const close = () => useStore.setState({ dialog: null });

  return (
    <Modal open={open} onClose={close} label="Settings" className="dialog-settings">
      <header className="dialog-head">
        <h2>Settings</h2>
        <button type="button" className="icon-button" aria-label="Close settings" onClick={close}>
          <Icon name="close" />
        </button>
      </header>
      <div className="settings-layout">
        <nav className="settings-tabs" role="tablist" aria-label="Settings sections">
          {TABS.map((t) => (
            <button
              key={t.id}
              type="button"
              role="tab"
              aria-selected={tab === t.id}
              className={tab === t.id ? 'is-active' : ''}
              onClick={() => useStore.setState({ settingsTab: t.id })}
              data-autofocus={tab === t.id ? true : undefined}
            >
              {t.label}
            </button>
          ))}
        </nav>
        <div className="settings-panel" role="tabpanel">
          {tab === 'audio' && <AudioTab />}
          {tab === 'hotkeys' && <HotkeysTab />}
          {tab === 'app' && <AppTab />}
          {tab === 'about' && <AboutTab />}
        </div>
      </div>
    </Modal>
  );
}

function AudioTab() {
  const audio = useStore((s) => s.settings.audio);
  const devices = useStore((s) => s.devices);
  const platform = useStore((s) => s.platform);
  const status = useStore((s) => s.virtualStatus);
  const label = micLabel(status);

  return (
    <div className="settings-section">
      <section className="settings-group">
        <h3>Where your sounds go</h3>
        {platform === 'windows' ? (
          <DeviceSelect
            label="Virtual cable"
            value={audio.virtualOutput}
            devices={devices.outputs}
            defaultLabel={devices.virtualCandidates.length ? 'Detect automatically' : 'Detect automatically (none found)'}
            onChange={(virtualOutput) => updateAudio({ virtualOutput })}
            hint={
              devices.virtualCandidates.length === 0 ? (
                <>
                  No virtual cable found.{' '}
                  <button type="button" className="link" onClick={() => void openUrl(VB_CABLE_URL)}>
                    Get VB-CABLE (free)
                  </button>
                </>
              ) : (
                'Kavox plays into this device. Its other end shows up as a microphone in voice apps.'
              )
            }
          />
        ) : (
          <div className="field">
            <span className="field-label">Virtual microphone</span>
            <p className="field-static">{status.kind === 'failed' ? status.message : 'Kavox Mic, created while Kavox runs'}</p>
          </div>
        )}
        {label && (
          <p className="callout">
            In Discord, OBS or your game, set the microphone to <strong>{label}</strong>.
          </p>
        )}
      </section>

      <section className="settings-group">
        <h3>Your microphone</h3>
        <Switch
          label="Send my voice through Kavox"
          description="Mixes your real mic in with the sounds. Turn off if you only want the sounds, or mix your voice elsewhere."
          checked={audio.micPassthrough}
          onChange={(micPassthrough) => updateAudio({ micPassthrough })}
        />
        <DeviceSelect
          label="Microphone"
          value={audio.micDevice}
          devices={devices.inputs}
          onChange={(micDevice) => updateAudio({ micDevice })}
        />
      </section>

      <section className="settings-group">
        <h3>Hearing your sounds</h3>
        <Switch
          label="Play sounds in my headphones too"
          checked={audio.monitorEnabled}
          onChange={(monitorEnabled) => updateAudio({ monitorEnabled })}
        />
        <DeviceSelect
          label="Headphones or speakers"
          value={audio.monitorOutput}
          devices={devices.outputs.filter((d) => !devices.virtualCandidates.includes(d.id))}
          onChange={(monitorOutput) => updateAudio({ monitorOutput })}
        />
        <div className="button-row">
          <button type="button" className="button" onClick={() => void run(api.playTestTone('monitor'))}>
            <Icon name="headphones" />
            Play test tone
          </button>
          <button type="button" className="button" onClick={() => void run(api.refreshDevices())}>
            Rescan devices
          </button>
        </div>
      </section>

      <section className="settings-group">
        <h3>Volume</h3>
        <Switch
          label="Even out loudness"
          description="Brings quiet and loud files to a similar level when they play. Each sound's own volume still applies."
          checked={audio.normalize}
          onChange={(normalize) => updateAudio({ normalize })}
        />
      </section>

      <button type="button" className="button" onClick={() => useStore.setState({ dialog: 'wizard' })}>
        Run setup again
      </button>
    </div>
  );
}

function HotkeysTab() {
  const hotkeys = useStore((s) => s.settings.hotkeys);
  const supported = useStore((s) => s.hotkeysSupported);

  return (
    <div className="settings-section">
      {!supported && (
        <p className="callout is-warn">
          You're on Wayland, where apps can't register global hotkeys. Kavox saves the keys below, and you can bind
          kavoxctl commands in your compositor instead.
        </p>
      )}
      <section className="settings-group">
        <h3>Global hotkeys</h3>
        <p className="field-hint">Set a hotkey for each sound in its editor (right-click a pad).</p>
        <HotkeyRecorder label="Stop all sounds" value={hotkeys.stopAll} onChange={(h) => api.setGlobalHotkey('stopAll', h)} />
        <HotkeyRecorder label="Mute or unmute mic" value={hotkeys.toggleMute} onChange={(h) => api.setGlobalHotkey('toggleMute', h)} />
      </section>
      <KavoxctlGuide />
    </div>
  );
}

export function KavoxctlGuide() {
  const sounds = useStore((s) => s.library.sounds);
  const example = Object.values(sounds)[0]?.name ?? 'airhorn';
  const quoted = example.includes(' ') ? `"${example}"` : example;
  const [compositor, setCompositor] = useState<'niri' | 'hyprland' | 'sway' | 'other'>('niri');

  const snippets = {
    niri: `// ~/.config/niri/config.kdl, inside binds { }\nMod+F1 { spawn "kavoxctl" "play" "${example}"; }\nMod+F12 { spawn "kavoxctl" "stop-all"; }`,
    hyprland: `# ~/.config/hypr/hyprland.conf\nbind = , F13, exec, kavoxctl play ${quoted}\nbind = , F24, exec, kavoxctl stop-all`,
    sway: `# ~/.config/sway/config\nbindsym F13 exec kavoxctl play ${quoted}\nbindsym F24 exec kavoxctl stop-all`,
    other: `# KDE: System Settings → Shortcuts → Add Command\n# GNOME: Settings → Keyboard → Custom Shortcuts\nkavoxctl play ${quoted}`,
  };

  return (
    <section className="settings-group">
      <h3>Control Kavox from anywhere</h3>
      <p className="field-hint">
        kavoxctl talks to the running app, so any key binding, script or stream deck button can play sounds. It matches
        names exactly first, then by the start of the name.
      </p>
      <Segmented
        label="Desktop"
        value={compositor}
        onChange={setCompositor}
        options={[
          { value: 'niri', label: 'niri' },
          { value: 'hyprland', label: 'Hyprland' },
          { value: 'sway', label: 'Sway' },
          { value: 'other', label: 'KDE, GNOME' },
        ]}
      />
      <CodeBlock code={snippets[compositor]} />
      <p className="field-hint">
        Also: <code>kavoxctl stop {quoted}</code>, <code>kavoxctl mute</code> (toggles), <code>kavoxctl mute on</code>,{' '}
        <code>kavoxctl list</code>.
      </p>
    </section>
  );
}

function CodeBlock({ code }: { code: string }) {
  return (
    <div className="code-block">
      <pre>
        <code>{code}</code>
      </pre>
      <button
        type="button"
        className="icon-button"
        aria-label="Copy"
        onClick={() =>
          void navigator.clipboard.writeText(code).then(
            () => toast('info', 'Copied'),
            () => toast('error', 'Couldn’t copy to the clipboard'),
          )
        }
      >
        <Icon name="copy" size={16} />
      </button>
    </div>
  );
}

function AppTab() {
  const app = useStore((s) => s.settings.app);
  return (
    <div className="settings-section">
      <section className="settings-group">
        <h3>Window</h3>
        <Switch
          label="Keep running when I close the window"
          description="Kavox stays in the tray so hotkeys keep working. Open it again from the tray or by launching Kavox."
          checked={app.closeToTray}
          onChange={(closeToTray) => void updateApp({ closeToTray })}
        />
        <Switch
          label="Start hidden in the tray"
          checked={app.startMinimized}
          onChange={(startMinimized) => void updateApp({ startMinimized })}
        />
        <Switch
          label="Start Kavox when I log in"
          checked={app.launchOnLogin}
          onChange={(launchOnLogin) => void updateApp({ launchOnLogin })}
        />
      </section>
      <section className="settings-group">
        <h3>Appearance</h3>
        <Segmented<Theme>
          label="Theme"
          value={app.theme}
          onChange={(theme) => void updateApp({ theme })}
          options={[
            { value: 'system', label: 'Match system' },
            { value: 'dark', label: 'Dark' },
            { value: 'light', label: 'Light' },
          ]}
        />
      </section>
    </div>
  );
}

function AboutTab() {
  const version = useStore((s) => s.version);
  const platform = useStore((s) => s.platform);

  const copyDiagnostics = async () => {
    const text = await run(api.copyDiagnostics());
    if (text === undefined) return;
    await navigator.clipboard.writeText(text).then(
      () => toast('info', 'Diagnostics copied. Paste them into your bug report.'),
      () => toast('error', 'Couldn’t copy to the clipboard'),
    );
  };

  return (
    <div className="settings-section about">
      <div className="about-head">
        <LogoMark size={48} />
        <div>
          <h3 className="about-name">Kavox</h3>
          <p className="field-hint">
            Version {version} for {platform === 'windows' ? 'Windows' : platform === 'linux' ? 'Linux' : platform}. Free, no
            account, no tracking.
          </p>
        </div>
      </div>
      <div className="button-row">
        <button type="button" className="button" onClick={() => void copyDiagnostics()}>
          <Icon name="copy" />
          Copy diagnostics
        </button>
        <button type="button" className="button" onClick={() => void run(api.openLogsFolder())}>
          <Icon name="folder" />
          Open logs folder
        </button>
      </div>
      <div className="button-row">
        <button type="button" className="button" onClick={() => void openUrl(REPO_URL)}>
          <Icon name="external" />
          Source code and issues
        </button>
        {platform === 'windows' && (
          <button type="button" className="button" onClick={() => void openUrl(VB_CABLE_URL)}>
            <Icon name="external" />
            VB-CABLE website
          </button>
        )}
      </div>
    </div>
  );
}
