import { api } from '../api';
import { dismissToast, openSettings, run, useStore } from '../store';
import { Icon, Modal } from './ui';

/** Shown only when sounds can't reach voice apps, or Kavox can't save. */
export function StatusBanner() {
  const status = useStore((s) => s.virtualStatus);
  const platform = useStore((s) => s.platform);
  const readOnly = useStore((s) => s.readOnly);

  let tone: 'bad' | 'warn' = 'bad';
  let message: string;
  let action: { label: string; onClick: () => void } | null = null;

  switch (status.kind) {
    case 'ok':
      if (!readOnly) return null;
      tone = 'warn';
      message = 'Your sounds and settings were saved by a newer Kavox, so changes made here won’t be saved. Update Kavox to edit them.';
      break;
    case 'notInstalled':
      message = 'Voice apps can’t hear Kavox yet. Install the free VB-CABLE driver so Discord and games get a Kavox microphone.';
      action = { label: 'Set up', onClick: () => useStore.setState({ dialog: 'wizard' }) };
      break;
    case 'missing':
      message = `${status.device} isn’t connected, so sounds aren’t reaching your voice apps.`;
      action = { label: 'Choose another', onClick: () => openSettings('audio') };
      break;
    case 'cableIsDefaultOutput':
      tone = 'warn';
      message = `${status.device} is your default speaker, so everything your PC plays goes into your mic. Pick your real speakers or headphones as the default.`;
      action = platform === 'windows' ? { label: 'Open Sound settings', onClick: () => void run(api.openSoundSettings()) } : null;
      break;
    case 'failed':
      message = status.message;
      action = { label: 'Audio settings', onClick: () => openSettings('audio') };
      break;
  }

  return (
    <div className={`banner is-${tone}`} role="status">
      <Icon name="warning" />
      <p>{message}</p>
      {action && (
        <button type="button" className="button" onClick={action.onClick}>
          {action.label}
        </button>
      )}
    </div>
  );
}

export function Toasts() {
  const toasts = useStore((s) => s.toasts);
  return (
    <div className="toasts" aria-live="polite">
      {toasts.map((t) => (
        <div key={t.id} className={`toast is-${t.level}`} role={t.level === 'error' ? 'alert' : 'status'}>
          <Icon name={t.level === 'info' ? 'check' : t.level === 'warn' ? 'warning' : 'info'} />
          <p>{t.message}</p>
          <button type="button" className="icon-button" aria-label="Dismiss" onClick={() => dismissToast(t.id)}>
            <Icon name="close" size={14} />
          </button>
        </div>
      ))}
    </div>
  );
}

export function ConfirmDialog() {
  const confirm = useStore((s) => s.confirm);
  const close = () => useStore.setState({ confirm: null });
  return (
    <Modal open={confirm !== null} onClose={close} label={confirm?.title ?? ''} className="dialog-small">
      {confirm && (
        <>
          <div className="dialog-body">
            <h2>{confirm.title}</h2>
            <p>{confirm.message}</p>
          </div>
          <div className="dialog-actions">
            <button type="button" className="button" onClick={close} data-autofocus>
              Cancel
            </button>
            <button
              type="button"
              className="button button-danger"
              onClick={() => {
                close();
                confirm.onConfirm();
              }}
            >
              {confirm.action}
            </button>
          </div>
        </>
      )}
    </Modal>
  );
}

export function DropOverlay() {
  const dragging = useStore((s) => s.dragging);
  const boardName = useStore((s) => s.library.boards.find((b) => b.id === s.library.activeBoard)?.name);
  if (!dragging) return null;
  return (
    <div className="drop-overlay" aria-hidden="true">
      <div className="drop-target">
        <Icon name="plus" size={32} />
        <p>Drop to add to {boardName}</p>
      </div>
    </div>
  );
}

export function LoadError({ message }: { message: string }) {
  return (
    <div className="load-error">
      <h1>Kavox couldn't start</h1>
      <p>{message}</p>
      <button type="button" className="button button-primary" onClick={() => location.reload()}>
        Try again
      </button>
    </div>
  );
}
