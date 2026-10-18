import { useState, type ReactNode } from 'react';
import { openUrl } from '@tauri-apps/plugin-opener';
import { api, VB_CABLE_URL } from '../api';
import { micLabel, run, updateApp, updateAudio, useStore } from '../store';
import { KavoxctlGuide } from './Settings';
import { DeviceSelect, Icon, LevelBar, LogoMark, Modal, Switch } from './ui';

type Step = 'cable' | 'devices' | 'test' | 'apps';
const STEPS: { id: Step; title: string }[] = [
  { id: 'cable', title: 'Virtual microphone' },
  { id: 'devices', title: 'Your devices' },
  { id: 'test', title: 'Test' },
  { id: 'apps', title: 'Voice apps' },
];

export function SetupWizard() {
  const open = useStore((s) => s.dialog === 'wizard');
  const [index, setIndex] = useState(0);
  const step = STEPS[index];

  const finish = () => {
    useStore.setState({ dialog: null });
    setIndex(0);
    void updateApp({ setupComplete: true });
  };

  return (
    <Modal open={open} onClose={finish} label="Set up Kavox" className="dialog-wizard">
      <header className="wizard-head">
        <LogoMark size={32} />
        <div>
          <h2>Set up Kavox</h2>
          <ol className="wizard-steps" aria-label="Steps">
            {STEPS.map((s, i) => (
              <li key={s.id} className={i === index ? 'is-current' : i < index ? 'is-done' : ''} aria-current={i === index ? 'step' : undefined}>
                <span className="wizard-step-number">{i < index ? <Icon name="check" size={12} /> : i + 1}</span>
                <span className="wizard-step-title">{s.title}</span>
              </li>
            ))}
          </ol>
        </div>
      </header>

      <div className="wizard-body">
        {step.id === 'cable' && <CableStep />}
        {step.id === 'devices' && <DevicesStep />}
        {step.id === 'test' && <TestStep />}
        {step.id === 'apps' && <AppsStep />}
      </div>

      <footer className="dialog-actions">
        <button type="button" className="button button-quiet" onClick={finish}>
          {index === STEPS.length - 1 ? 'Close' : 'Skip setup'}
        </button>
        <span className="spacer" />
        {index > 0 && (
          <button type="button" className="button" onClick={() => setIndex(index - 1)}>
            Back
          </button>
        )}
        {index < STEPS.length - 1 ? (
          <button type="button" className="button button-primary" onClick={() => setIndex(index + 1)} data-autofocus>
            Next
          </button>
        ) : (
          <button type="button" className="button button-primary" onClick={finish} data-autofocus>
            Finish
          </button>
        )}
      </footer>
    </Modal>
  );
}

function StatusLine({ tone, children }: { tone: 'ok' | 'warn' | 'bad'; children: ReactNode }) {
  return (
    <p className={`status-line is-${tone}`}>
      <Icon name={tone === 'ok' ? 'check' : 'warning'} />
      <span>{children}</span>
    </p>
  );
}

function CableStep() {
  const platform = useStore((s) => s.platform);
  const status = useStore((s) => s.virtualStatus);
  const [checking, setChecking] = useState(false);

  const check = async () => {
    setChecking(true);
    await run(api.refreshDevices());
    setChecking(false);
  };

  if (platform !== 'windows') {
    return (
      <div className="wizard-step">
        <h3>Kavox makes its own microphone</h3>
        <p>
          While Kavox runs, your system has an extra input called <strong>Kavox Mic</strong>. Voice apps that use it hear
          your sounds mixed with your voice. It disappears when you quit Kavox.
        </p>
        {status.kind === 'failed' ? (
          <StatusLine tone="bad">
            {status.message}. Kavox needs PulseAudio, or PipeWire with pipewire-pulse.
          </StatusLine>
        ) : (
          <StatusLine tone="ok">Kavox Mic is ready.</StatusLine>
        )}
      </div>
    );
  }

  return (
    <div className="wizard-step">
      <h3>Give Kavox a way into your voice apps</h3>
      <p>
        Windows apps can't create microphones on their own, so Kavox plays into a free virtual cable called VB-CABLE.
        Discord and games then pick the cable's other end as their microphone.
      </p>

      {status.kind === 'notInstalled' && (
        <>
          <StatusLine tone="bad">No virtual cable found yet.</StatusLine>
          <ol className="steps-list">
            <li>
              <button type="button" className="link" onClick={() => void openUrl(VB_CABLE_URL)}>
                Download VB-CABLE
              </button>{' '}
              and unzip it.
            </li>
            <li>Right-click VBCABLE_Setup_x64.exe, choose Run as administrator, then click Install Driver.</li>
            <li>Restart your PC if the installer asks you to.</li>
          </ol>
        </>
      )}
      {status.kind === 'cableIsDefaultOutput' && (
        <>
          <StatusLine tone="warn">
            Found {status.device}, but it became your default speaker. Everything your PC plays would go into your mic.
          </StatusLine>
          <p>Open Sound settings and choose your real speakers or headphones as the output device.</p>
          <button type="button" className="button" onClick={() => void run(api.openSoundSettings())}>
            <Icon name="external" />
            Open Sound settings
          </button>
        </>
      )}
      {status.kind === 'ok' && <StatusLine tone="ok">Found {status.device}. You're set.</StatusLine>}
      {status.kind === 'missing' && <StatusLine tone="bad">{status.device} isn't connected.</StatusLine>}
      {status.kind === 'failed' && <StatusLine tone="bad">{status.message}</StatusLine>}

      {status.kind !== 'ok' && (
        <button type="button" className="button" onClick={() => void check()} disabled={checking}>
          {checking ? 'Checking…' : 'Check again'}
        </button>
      )}
    </div>
  );
}

function DevicesStep() {
  const audio = useStore((s) => s.settings.audio);
  const devices = useStore((s) => s.devices);
  const level = useStore((s) => s.levels.mic);

  return (
    <div className="wizard-step">
      <h3>Pick your mic and headphones</h3>
      <DeviceSelect label="Microphone" value={audio.micDevice} devices={devices.inputs} onChange={(micDevice) => updateAudio({ micDevice })} />
      <LevelBar level={level} label="Microphone level" />
      <p className="field-hint">Say something; the bar should move.</p>
      <Switch
        label="Send my voice through Kavox"
        description="Leave this on so friends hear you and your sounds through one microphone."
        checked={audio.micPassthrough}
        onChange={(micPassthrough) => updateAudio({ micPassthrough })}
      />
      <DeviceSelect
        label="Headphones or speakers"
        value={audio.monitorOutput}
        devices={devices.outputs.filter((d) => !devices.virtualCandidates.includes(d.id))}
        onChange={(monitorOutput) => updateAudio({ monitorOutput })}
      />
      <button type="button" className="button" onClick={() => void run(api.playTestTone('monitor'))}>
        <Icon name="headphones" />
        Play test tone in headphones
      </button>
    </div>
  );
}

function TestStep() {
  const level = useStore((s) => s.levels.virtual);
  const status = useStore((s) => s.virtualStatus);
  const label = micLabel(status);

  return (
    <div className="wizard-step">
      <h3>Send a test tone into the mic</h3>
      <p>
        This plays a short tone into {label ? <strong>{label}</strong> : 'the virtual microphone'}, the way your sounds will
        reach voice apps. You won't hear it unless an app is listening to that mic.
      </p>
      <button type="button" className="button button-primary" onClick={() => void run(api.playTestTone('virtual'))} disabled={!label}>
        <Icon name="broadcast" />
        Play test tone into mic
      </button>
      <LevelBar level={level} label="Virtual microphone level" />
      {!label && <StatusLine tone="bad">Finish the first step so Kavox has a microphone to play into.</StatusLine>}
      <p className="field-hint">To check from the other end, use the mic test in Discord's Voice & Video settings.</p>
    </div>
  );
}

function AppsStep() {
  const status = useStore((s) => s.virtualStatus);
  const hotkeysSupported = useStore((s) => s.hotkeysSupported);
  const label = micLabel(status) ?? 'the Kavox microphone';

  return (
    <div className="wizard-step">
      <h3>Point your voice apps at Kavox</h3>
      <div className="app-card">
        <h4>Discord</h4>
        <ol className="steps-list">
          <li>
            Open User Settings, then Voice & Video, and set Input Device to <strong>{label}</strong>.
          </li>
          <li>Set Noise Suppression to None. Otherwise Discord filters out anything that isn't a voice.</li>
          <li>Turn off Echo Cancellation and Automatic Gain Control.</li>
          <li>If you use voice activity, set the sensitivity by hand and keep it low so quiet sounds get through.</li>
        </ol>
      </div>
      <div className="app-card">
        <h4>OBS and games</h4>
        <p>
          Choose <strong>{label}</strong> wherever the app asks for a microphone. In OBS, add an Audio Input Capture source
          with that device.
        </p>
      </div>
      {!hotkeysSupported && <KavoxctlGuide />}
    </div>
  );
}
