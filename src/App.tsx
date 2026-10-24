import { useEffect } from 'react';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { importPaths, init, useStore } from './store';
import { ConfirmDialog, DropOverlay, LoadError, StatusBanner, Toasts } from './components/Feedback';
import { MixerStrip } from './components/MixerStrip';
import { PadEditor } from './components/PadEditor';
import { PadGrid } from './components/Pads';
import { VoiceBoard } from './components/VoiceBoard';
import { SettingsDialog } from './components/Settings';
import { SetupWizard } from './components/SetupWizard';
import { TopBar } from './components/TopBar';

export function App() {
  const ready = useStore((s) => s.ready);
  const loadError = useStore((s) => s.loadError);
  const theme = useStore((s) => s.settings.app?.theme);
  const editing = useStore((s) => s.editing !== null);
  const view = useStore((s) => s.view);

  useEffect(() => {
    void init();
  }, []);

  useEffect(() => {
    if (theme === 'light' || theme === 'dark') document.documentElement.dataset.theme = theme;
    else delete document.documentElement.dataset.theme;
  }, [theme]);

  useEffect(() => {
    const unlisten = getCurrentWebview().onDragDropEvent((event) => {
      const { type } = event.payload;
      if (type === 'enter' || type === 'over') useStore.setState({ dragging: true });
      else if (type === 'leave') useStore.setState({ dragging: false });
      else if (type === 'drop') {
        useStore.setState({ dragging: false });
        void importPaths(event.payload.paths);
      }
    });
    return () => void unlisten.then((fn) => fn());
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'f') {
        e.preventDefault();
        document.getElementById('sound-search')?.focus();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  if (loadError) return <LoadError message={loadError} />;
  if (!ready) return <div className="splash" aria-busy="true" />;

  return (
    <div className={`app ${editing ? 'is-editing' : ''}`}>
      <TopBar />
      <StatusBanner />
      <main className="board">
        {view === 'voice' ? <VoiceBoard /> : <PadGrid />}
      </main>
      <PadEditor />
      <MixerStrip />
      <SettingsDialog />
      <SetupWizard />
      <ConfirmDialog />
      <Toasts />
      <DropOverlay />
    </div>
  );
}
