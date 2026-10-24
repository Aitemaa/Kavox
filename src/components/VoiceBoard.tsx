import type { CSSProperties } from 'react';
import { api } from '../api';
import { updateVoice, useStore } from '../store';
import { CUSTOM, NORMAL, VOICES, formatSemitones, voiceName, type VoiceInfo } from '../voices';
import { HotkeyRecorder, Icon, Keycaps, Slider, Switch } from './ui';

/** The Voice view: pick a voice, nudge the pitch, and everyone on the call hears it. */
export function VoiceBoard() {
  const voice = useStore((s) => s.settings.voice);
  const supported = useStore((s) => s.hotkeysSupported);
  const live = voice.enabled && voice.preset !== NORMAL;
  const dial = voice.preset === CUSTOM ? voice.semitones : (VOICES.find((v) => v.id === voice.preset)?.semitones ?? 0);

  return (
    <section className={`voice-board ${live ? 'is-live' : ''}`} aria-label="Voice changer">
      <header className="voice-head">
        <div>
          <h2>Voice changer</h2>
          <p>
            {live
              ? `You sound like ${voiceName(voice.preset)}. Everyone on the call hears it.`
              : 'Pick a voice. Only your mic changes: the sounds on your pads stay as they are.'}
          </p>
        </div>
        <Switch label="Voice changer" checked={voice.enabled} onChange={(enabled) => void updateVoice({ enabled })} />
      </header>

      <div className="voice-grid" role="list">
        <NormalTile selected={voice.preset === NORMAL || !voice.enabled} />
        {VOICES.map((v) => (
          <VoiceTile key={v.id} info={v} hotkey={voice.hotkeys[v.id]} />
        ))}
      </div>

      <div className="voice-controls">
        <div className="field">
          <div className="field-row">
            <span className="field-label">Pitch</span>
            <span className="field-value">{formatSemitones(dial)}</span>
          </div>
          <Slider
            label="Pitch in semitones"
            value={dial}
            min={-12}
            max={12}
            step={0.5}
            center={0}
            valueText={formatSemitones(dial)}
            onChange={(semitones) => void updateVoice({ preset: CUSTOM, semitones, enabled: true })}
          />
          <p className="field-hint">Drag to make your own voice. Low is deeper, high is squeakier.</p>
        </div>
        <Switch
          label="Hear myself"
          description="Plays your changed voice in your headphones. Use headphones, or the mic will pick it up."
          checked={voice.hearMyself}
          onChange={(hearMyself) => void updateVoice({ hearMyself })}
        />
      </div>

      <details className="voice-hotkeys">
        <summary>Hotkeys</summary>
        <p className="field-hint">
          Press a voice's key to switch to it, and press it again to go back to your normal voice.
          {!supported && ' Global hotkeys aren’t available on Wayland, so these only save for later.'}
        </p>
        <div className="voice-hotkey-grid">
          {VOICES.map((v) => (
            <HotkeyRecorder
              key={v.id}
              label={v.name}
              value={voice.hotkeys[v.id] ?? null}
              onChange={(hotkey) => api.setVoiceHotkey(v.id, hotkey)}
            />
          ))}
        </div>
      </details>
    </section>
  );
}

function NormalTile({ selected }: { selected: boolean }) {
  return (
    <div role="listitem" className={`voice-slot ${selected ? 'is-selected' : ''}`} style={{ '--pad': 'var(--faint)' } as CSSProperties}>
      <button
        type="button"
        className="voice-tile"
        aria-pressed={selected}
        onClick={() => void updateVoice({ preset: NORMAL, enabled: false })}
      >
        <span className="pad-dot" aria-hidden="true" />
        <Icon name="mic" size={26} />
        <span className="pad-name">Normal</span>
        <span className="pad-foot">
          <span />
          <span className="pad-info">As you are</span>
        </span>
      </button>
    </div>
  );
}

function VoiceTile({ info, hotkey }: { info: VoiceInfo; hotkey?: string }) {
  const selected = useStore((s) => s.settings.voice.enabled && s.settings.voice.preset === info.id);
  const style = { '--pad': info.color } as CSSProperties;
  return (
    <div role="listitem" className={`voice-slot ${selected ? 'is-selected' : ''}`} style={style}>
      <button
        type="button"
        className="voice-tile"
        aria-pressed={selected}
        aria-label={`${info.name}${hotkey ? `, hotkey ${hotkey}` : ''}`}
        onClick={() => void updateVoice({ preset: info.id, enabled: !selected })}
      >
        <span className="pad-dot" aria-hidden="true" />
        <span className="voice-icon">
          <Icon name={info.icon} size={26} />
        </span>
        <span className="pad-name">{info.name}</span>
        <span className="pad-foot">
          {hotkey ? <Keycaps hotkey={hotkey} small /> : <span />}
          <span className="pad-info">{info.wobble ? 'Wobbles' : formatSemitones(info.semitones)}</span>
        </span>
      </button>
    </div>
  );
}
