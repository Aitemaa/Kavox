import { api } from '../api';
import { micLabel, openSettings, run, updateAudio, useStore } from '../store';
import { Fader, Icon, Keycaps, type IconName } from './ui';

/** Bottom strip, laid out the way the signal flows: your mic, sounds into the mic, sounds in your ears, then where it goes. */
export function MixerStrip() {
  const audio = useStore((s) => s.settings.audio);
  const levels = useStore((s) => s.levels);
  const stopAllHotkey = useStore((s) => s.settings.hotkeys.stopAll);
  const micOff = audio.micMuted || !audio.micPassthrough;

  return (
    <footer className="strip">
      <div className="strip-channels">
        <div className={`channel ${micOff ? 'is-off' : ''}`}>
          <button
            type="button"
            className={`channel-icon mute-button ${audio.micMuted ? 'is-muted' : ''}`}
            aria-pressed={audio.micMuted}
            aria-label={audio.micMuted ? 'Unmute mic' : 'Mute mic'}
            title={audio.micMuted ? 'Mic muted: sounds still play' : 'Mute mic'}
            onClick={() => void run(api.setMicMuted())}
          >
            <Icon name={audio.micMuted ? 'micOff' : 'mic'} />
          </button>
          <Channel
            label={!audio.micPassthrough ? 'Mic (not sent)' : audio.micMuted ? 'Mic (muted)' : 'Your mic'}
            value={audio.micGain}
            level={levels.mic}
            onChange={(micGain) => updateAudio({ micGain })}
          />
        </div>
        <div className="channel">
          <ChannelIcon name="broadcast" />
          <Channel
            label="Sounds to mic"
            value={audio.soundsToMicGain}
            level={levels.virtual}
            onChange={(soundsToMicGain) => updateAudio({ soundsToMicGain })}
          />
        </div>
        <div className={`channel ${audio.monitorEnabled ? '' : 'is-off'}`}>
          <ChannelIcon name="headphones" />
          <Channel
            label={audio.monitorEnabled ? 'Sounds in your ears' : 'Your ears (off)'}
            value={audio.soundsToMonitorGain}
            level={levels.monitor}
            onChange={(soundsToMonitorGain) => updateAudio({ soundsToMonitorGain })}
          />
        </div>
      </div>

      <div className="strip-end">
        <button type="button" className="button stop-all" onClick={() => void run(api.stopAll())}>
          <Icon name="stop" size={14} />
          Stop all
          {stopAllHotkey && <Keycaps hotkey={stopAllHotkey} small />}
        </button>
        <OutputStatus />
      </div>
    </footer>
  );
}

function ChannelIcon({ name }: { name: IconName }) {
  return (
    <span className="channel-icon" aria-hidden="true">
      <Icon name={name} />
    </span>
  );
}

function Channel({ label, value, level, onChange }: { label: string; value: number; level: number; onChange: (v: number) => void }) {
  return (
    <div className="channel-body">
      <div className="channel-label">
        <span>{label}</span>
        <span className="channel-value">{Math.round(value * 100)}%</span>
      </div>
      <Fader label={label} value={value} max={2} level={level} onChange={onChange} />
    </div>
  );
}

function OutputStatus() {
  const status = useStore((s) => s.virtualStatus);
  const streams = useStore((s) => s.streams);
  const label = micLabel(status);
  const fallback = Boolean(streams.mic?.fallback || streams.monitor?.fallback);
  const tone = !label || !streams.virtual ? 'bad' : status.kind === 'cableIsDefaultOutput' || fallback ? 'warn' : 'ok';
  const short = label?.replace(/\s*\(.*\)\s*$/, '');
  const text = tone === 'bad' ? 'Voice apps can’t hear Kavox' : `Voice apps use ${short}`;

  return (
    <button
      type="button"
      className={`output-status is-${tone}`}
      title={label ? `Select "${label}" as the microphone in Discord, OBS or your game` : 'Open audio settings'}
      onClick={() => openSettings('audio')}
    >
      <span className="status-dot" aria-hidden="true" />
      <span className="output-status-text">{text}</span>
    </button>
  );
}
