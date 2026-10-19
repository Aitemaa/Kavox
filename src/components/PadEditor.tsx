import { useEffect, useId, useState, type CSSProperties } from 'react';
import { api, PALETTE, type Retrigger, type Sound, type SoundPatch } from '../api';
import { confirmDeleteSound, formatDuration, run, useStore } from '../store';
import { Fader, HotkeyRecorder, Icon, Segmented, Switch } from './ui';

const MODE_HINTS: Record<Retrigger, string> = {
  restart: 'Pressing again starts the sound over.',
  overlap: 'Pressing again plays another copy on top.',
  toggle: 'Pressing again stops it.',
};

export function PadEditor() {
  const sound = useStore((s) => (s.editing ? s.library.sounds[s.editing] : undefined));
  const blocked = useStore((s) => s.dialog !== null || s.confirm !== null);

  useEffect(() => {
    if (!sound || blocked) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') useStore.setState({ editing: null });
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [sound, blocked]);

  if (!sound) return null;
  return <Editor key={sound.id} sound={sound} />;
}

function Editor({ sound }: { sound: Sound }) {
  const hotkeysSupported = useStore((s) => s.hotkeysSupported);
  const [name, setName] = useState(sound.name);
  const [volume, setVolume] = useState(sound.volume);
  const nameId = useId();

  useEffect(() => setName(sound.name), [sound.name]);

  const patch = (p: SoundPatch) => run(api.updateSound(sound.id, p));
  const saveName = () => {
    const trimmed = name.trim();
    if (!trimmed) setName(sound.name);
    else if (trimmed !== sound.name) void patch({ name: trimmed });
  };
  const close = () => useStore.setState({ editing: null });

  return (
    <aside className="drawer" aria-label={`Edit ${sound.name}`} style={{ '--pad': sound.color } as CSSProperties}>
      <header className="drawer-head">
        <span className="drawer-dot" aria-hidden="true" />
        <h2>Edit sound</h2>
        <button type="button" className="icon-button" aria-label="Close" onClick={close}>
          <Icon name="close" />
        </button>
      </header>

      <div className="drawer-body">
        <div className="field">
          <label htmlFor={nameId}>Name</label>
          <input
            id={nameId}
            value={name}
            maxLength={60}
            onChange={(e) => setName(e.target.value)}
            onBlur={saveName}
            onKeyDown={(e) => e.key === 'Enter' && (e.target as HTMLInputElement).blur()}
          />
          <p className="field-hint">{formatDuration(sound.durationMs)} long. kavoxctl can play it by this name.</p>
        </div>

        <div className="field">
          <span className="field-label" id={`${nameId}-color`}>Color</span>
          <div className="swatches" role="radiogroup" aria-labelledby={`${nameId}-color`}>
            {PALETTE.map((color) => (
              <button
                key={color}
                type="button"
                role="radio"
                aria-checked={sound.color === color}
                aria-label={color}
                className="swatch"
                style={{ background: color }}
                onClick={() => void patch({ color })}
              />
            ))}
          </div>
        </div>

        <div className="field">
          <div className="field-row">
            <span className="field-label">Volume</span>
            <span className="field-value">{Math.round(volume * 100)}%</span>
          </div>
          <div className="volume-row">
            <Fader
              label="Volume"
              value={volume}
              max={2}
              onChange={(v) => {
                setVolume(v);
                void patch({ volume: v });
              }}
            />
            <button type="button" className="icon-button" aria-label={`Preview ${sound.name}`} onClick={() => void run(api.playSound(sound.id))}>
              <Icon name="play" />
            </button>
          </div>
        </div>

        <HotkeyRecorder label="Hotkey" value={sound.hotkey} onChange={(h) => api.setSoundHotkey(sound.id, h)} />
        {!hotkeysSupported && (
          <p className="field-hint">
            Global hotkeys don't work on Wayland. Bind <code>kavoxctl play "{sound.name}"</code> to a key in your compositor
            instead; Settings → Hotkeys shows how.
          </p>
        )}

        <div className="field">
          <span className="field-label">When pressed again</span>
          <Segmented<Retrigger>
            label="When pressed again"
            value={sound.retrigger}
            onChange={(retrigger) => void patch({ retrigger })}
            options={[
              { value: 'restart', label: 'Restart', icon: 'restart' },
              { value: 'overlap', label: 'Overlap', icon: 'overlap' },
              { value: 'toggle', label: 'Stop', icon: 'toggle' },
            ]}
          />
          <p className="field-hint">{MODE_HINTS[sound.retrigger]}</p>
        </div>

        <Switch
          label="Loop"
          description="Repeats until you stop it."
          checked={sound.loop}
          onChange={(loop) => void patch({ loop })}
        />
      </div>

      <footer className="drawer-foot">
        <button type="button" className="button button-danger" onClick={() => confirmDeleteSound(sound.id)}>
          <Icon name="trash" />
          Delete sound
        </button>
      </footer>
    </aside>
  );
}
