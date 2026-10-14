import React, { useCallback, useEffect, useRef, useState } from 'react';
import { AgentActionLog, DeviceState, PermissionRequest, Session, SessionMeta, SettingsView } from './lib/types';
import { subscribeAll, tauriApi } from './lib/tauri';
import { loadHistory, pushHistory } from './lib/history';
import { StatusBar } from './components/StatusBar';
import { PhoneScreen } from './components/PhoneScreen';
import { ActionLog } from './components/ActionLog';
import { ChatPanel } from './components/ChatPanel';
import { SettingsModal } from './components/SettingsModal';
import { SessionsPanel } from './components/SessionsPanel';
import './styles/index.css';

const SPLIT_KEY = 'vivy.split';
const CHATS_KEY = 'vivy.chatsOpen';
const PHONE_KEY = 'vivy.phoneOpen';
const SPLIT_MIN = 300;
const WORK_MIN = 420;

/** How long replaced messages take to fold away (matches .rewind-out in CSS). */
const REWIND_MS = 520;

const localLog = (action_type: AgentActionLog['action_type'], description: string): AgentActionLog => ({
  id: crypto.randomUUID(),
  timestamp: Date.now(),
  action_type,
  description,
});

function defaultSplit() {
  return Math.round(Math.max(SPLIT_MIN, window.innerWidth * 0.34));
}

function clampSplit(w: number) {
  return Math.round(Math.min(Math.max(w, SPLIT_MIN), Math.max(SPLIT_MIN, window.innerWidth - WORK_MIN)));
}

function loadSplit() {
  try {
    const v = Number(localStorage.getItem(SPLIT_KEY));
    return v > 0 ? clampSplit(v) : defaultSplit();
  } catch {
    return defaultSplit();
  }
}

export function App() {
  const [device, setDevice] = useState<DeviceState | null>(null);
  const [phoneError, setPhoneError] = useState<string | null>(null);
  const [logs, setLogs] = useState<AgentActionLog[]>([]);
  const [isRunning, setIsRunning] = useState(false);
  const [isPaused, setIsPaused] = useState(false);
  const [agentEnabled, setAgentEnabled] = useState(true);
  const [permissionReq, setPermissionReq] = useState<PermissionRequest | null>(null);
  const [isSettingsOpen, setIsSettingsOpen] = useState(false);
  const [settings, setSettings] = useState<SettingsView | null>(null);
  const [localIp, setLocalIp] = useState('127.0.0.1');
  const [history, setHistory] = useState<string[]>(loadHistory);
  const [split, setSplit] = useState(loadSplit);
  const [sessions, setSessions] = useState<SessionMeta[]>([]);
  const [activeSession, setActiveSession] = useState<{ id: string; title: string } | null>(null);
  const [phoneOpen, setPhoneOpen] = useState(() => {
    try {
      return localStorage.getItem(PHONE_KEY) !== '0';
    } catch {
      return true;
    }
  });
  const [chatsOpen, setChatsOpen] = useState(() => {
    try {
      return localStorage.getItem(CHATS_KEY) !== '0';
    } catch {
      return true;
    }
  });
  const dragging = useRef(false);
  const [draft, setDraft] = useState<{ text: string; n: number; rewindFrom?: string } | null>(null);
  // Message being replaced by a resend; it and what follows animate away.
  const [leavingFrom, setLeavingFrom] = useState<string | null>(null);

  const refreshSessions = useCallback(() => {
    tauriApi
      .listSessions()
      .then((list) => {
        setSessions(list);
        setActiveSession((cur) => {
          const match = cur && list.find((s) => s.id === cur.id);
          return match ? { id: match.id, title: match.title } : cur;
        });
      })
      .catch((err) => console.error('List chats error:', err));
  }, []);

  const showSession = (s: Session) => {
    setActiveSession({ id: s.id, title: s.title });
    setLogs(s.logs ?? []);
  };

  const loadInitialData = useCallback(async () => {
    try {
      const [conn, s, dev, agent] = await Promise.all([
        tauriApi.getConnectionInfo(),
        tauriApi.getSettings(),
        tauriApi.getDeviceStatus(),
        tauriApi.getAgentStatus(),
      ]);
      setLocalIp(conn.ip);
      setSettings(s);
      setDevice(dev.connected ? dev : null);
      setAgentEnabled(agent.enabled);
      setIsRunning(agent.running);
      setIsPaused(agent.paused);
      if (!agent.running) {
        showSession(await tauriApi.getActiveSession());
      }
      refreshSessions();
    } catch (err) {
      console.error('Initialization error:', err);
    }
  }, []);

  useEffect(() => {
    loadInitialData();

    const endRun = () => {
      setIsRunning(false);
      setIsPaused(false);
      setPermissionReq(null);
    };

    return subscribeAll([
      tauriApi.onPhoneConnected((d) => {
        setDevice(d);
        setPhoneError(null);
      }),
      tauriApi.onPhoneStatus((d) => setDevice(d.connected ? d : null)),
      tauriApi.onPhoneDisconnected(() => setDevice(null)),
      tauriApi.onPhoneError((msg) => setPhoneError(msg)),
      tauriApi.onAgentAction((log) => {
        setLogs((prev) => [...prev, log]);
        if (log.action_type === 'task') refreshSessions();
      }),
      tauriApi.onSessionsChanged(refreshSessions),
      tauriApi.onPermissionRequest((req) => setPermissionReq(req)),
      tauriApi.onPermissionCancelled(() => setPermissionReq(null)),
      tauriApi.onAgentComplete(endRun),
      tauriApi.onAgentError((e) => {
        endRun();
        // Errors raised before the loop starts (no key) never reach the feed otherwise.
        setLogs((prev) =>
          prev.length && prev[prev.length - 1].action_type === 'error'
            ? prev
            : [...prev, localLog('error', e.error)]
        );
      }),
      tauriApi.onAgentStopped(endRun),
      tauriApi.onAgentPaused(setIsPaused),
      tauriApi.onAgentEnabledChanged(setAgentEnabled),
    ]);
  }, [loadInitialData, refreshSessions]);

  // Keep the divider inside the window when it is resized.
  useEffect(() => {
    const onResize = () => setSplit((w) => clampSplit(w));
    window.addEventListener('resize', onResize);
    return () => window.removeEventListener('resize', onResize);
  }, []);

  const saveSplit = (w: number) => {
    try {
      localStorage.setItem(SPLIT_KEY, String(w));
    } catch {
      // not persisted; fine
    }
  };

  const onSplitPointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    dragging.current = true;
    e.currentTarget.setPointerCapture(e.pointerId);
    document.body.classList.add('resizing');
  };
  const onSplitPointerMove = (e: React.PointerEvent<HTMLDivElement>) => {
    if (dragging.current) setSplit(clampSplit(e.clientX));
  };
  const onSplitPointerUp = (e: React.PointerEvent<HTMLDivElement>) => {
    if (!dragging.current) return;
    dragging.current = false;
    e.currentTarget.releasePointerCapture(e.pointerId);
    document.body.classList.remove('resizing');
    saveSplit(clampSplit(e.clientX));
  };
  const onSplitKey = (e: React.KeyboardEvent<HTMLDivElement>) => {
    const delta = e.key === 'ArrowLeft' ? -24 : e.key === 'ArrowRight' ? 24 : 0;
    if (!delta) return;
    e.preventDefault();
    setSplit((w) => {
      const next = clampSplit(w + delta);
      saveSplit(next);
      return next;
    });
  };
  const resetSplit = () => {
    const w = clampSplit(defaultSplit());
    setSplit(w);
    saveSplit(w);
  };

  const handleSend = async (prompt: string, rewindFrom?: string) => {
    if (!settings?.has_key) {
      setIsSettingsOpen(true);
      return;
    }
    if (leavingFrom) return;
    setHistory(pushHistory(prompt));
    if (rewindFrom && logs.some((l) => l.id === rewindFrom)) {
      // Let the replaced messages fold away, then drop them before the new run starts.
      setLeavingFrom(rewindFrom);
      await new Promise((r) => window.setTimeout(r, REWIND_MS));
      setLogs((prev) => {
        const cut = prev.findIndex((l) => l.id === rewindFrom);
        return cut < 0 ? prev : prev.slice(0, cut);
      });
      setLeavingFrom(null);
    }
    setIsRunning(true);
    try {
      await tauriApi.startAgentTask(prompt, rewindFrom);
    } catch (err) {
      setIsRunning(false);
      // The chat may not have changed (e.g. a task was already running): show what is saved.
      if (rewindFrom) setLogs((await tauriApi.getActiveSession()).logs ?? []);
      setLogs((prev) => [...prev, localLog('error', String(err))]);
    }
    if (rewindFrom) refreshSessions();
  };

  const togglePhone = () => {
    const next = !phoneOpen;
    setPhoneOpen(next);
    try {
      localStorage.setItem(PHONE_KEY, next ? '1' : '0');
    } catch {
      // not persisted; fine
    }
  };

  const toggleChats = () => {
    const next = !chatsOpen;
    setChatsOpen(next);
    try {
      localStorage.setItem(CHATS_KEY, next ? '1' : '0');
    } catch {
      // not persisted; fine
    }
  };

  const chatAction = async (action: () => Promise<Session>) => {
    try {
      showSession(await action());
    } catch (err) {
      setLogs((prev) => [...prev, localLog('error', String(err))]);
    }
    refreshSessions();
  };

  const handleNewChat = () => chatAction(tauriApi.newSession);
  const handleOpenChat = (id: string) => chatAction(() => tauriApi.openSession(id));
  const handleDeleteChat = (id: string) => chatAction(() => tauriApi.deleteSession(id));
  const handleRenameChat = (id: string, title: string) => {
    tauriApi
      .renameSession(id, title)
      .then(refreshSessions)
      .catch((err) => console.error('Rename error:', err));
  };

  const handleEffortChange = async (reasoning_effort: string) => {
    setSettings((s) => (s ? { ...s, reasoning_effort } : s));
    try {
      await tauriApi.saveSettings({ reasoning_effort });
    } catch (err) {
      console.error('Effort change error:', err);
      loadInitialData();
    }
  };

  const handleModelChange = async (model_name: string) => {
    setSettings((s) => (s ? { ...s, model_name } : s));
    try {
      await tauriApi.saveSettings({ model_name });
    } catch (err) {
      console.error('Model change error:', err);
      loadInitialData();
    }
  };

  const handleStop = useCallback(() => {
    tauriApi.stopAgentTask().catch((err) => console.error('Stop error:', err));
  }, []);

  const handlePause = async (paused: boolean) => {
    setIsPaused(paused);
    try {
      setIsPaused(await tauriApi.setAgentPaused(paused));
    } catch (err) {
      console.error('Pause error:', err);
    }
  };

  const handleToggleAgent = async () => {
    const next = !agentEnabled;
    setAgentEnabled(next);
    try {
      await tauriApi.setAgentEnabled(next);
    } catch (err) {
      setAgentEnabled(!next);
      console.error('Toggle agent error:', err);
    }
  };

  // Esc stops the agent from anywhere in the window.
  useEffect(() => {
    if (!isRunning) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && !permissionReq && !isSettingsOpen) handleStop();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [isRunning, permissionReq, isSettingsOpen, handleStop]);

  const handleRespondPermission = async (id: string, allowed: boolean) => {
    setPermissionReq(null);
    try {
      await tauriApi.respondToPermission(id, allowed);
    } catch (err) {
      console.error('Permission response error:', err);
    }
  };

  const isConnected = Boolean(device?.connected);
  const hasKey = Boolean(settings?.has_key);

  return (
    <div className="app">
      <StatusBar
        device={device}
        localIp={localIp}
        hasKey={hasKey}
        agentEnabled={agentEnabled}
        isRunning={isRunning}
        isPaused={isPaused}
        onToggleAgent={handleToggleAgent}
        phoneOpen={phoneOpen}
        onTogglePhone={togglePhone}
        onOpenSettings={() => setIsSettingsOpen(true)}
      />

      <main
        className="main"
        style={{ gridTemplateColumns: phoneOpen ? `${split}px 7px minmax(0, 1fr)` : 'minmax(0, 1fr)' }}
      >
        {phoneOpen && (
          <PhoneScreen
            device={device}
            connectionUrl={`${localIp}:8765`}
            localIp={localIp}
            agentActive={isRunning && !isPaused}
            phoneError={phoneError}
          />
        )}

        {phoneOpen && <div
          className="splitter"
          role="separator"
          aria-orientation="vertical"
          aria-label="Resize phone and chat panels"
          aria-valuenow={split}
          tabIndex={0}
          title="Drag to resize · double-click to reset"
          onPointerDown={onSplitPointerDown}
          onPointerMove={onSplitPointerMove}
          onPointerUp={onSplitPointerUp}
          onDoubleClick={resetSplit}
          onKeyDown={onSplitKey}
        />}

        <section className="work-pane">
          {chatsOpen && (
            <SessionsPanel
              sessions={sessions}
              activeId={activeSession?.id ?? null}
              disabled={isRunning}
              onNew={handleNewChat}
              onOpen={handleOpenChat}
              onRename={handleRenameChat}
              onDelete={handleDeleteChat}
            />
          )}
          <div className="chat-col">
          <ActionLog
            logs={logs}
            isRunning={isRunning}
            isPaused={isPaused}
            canRun={agentEnabled && !isRunning}
            title={activeSession?.title ?? 'New chat'}
            chatsOpen={chatsOpen}
            onToggleChats={toggleChats}
            onNewChat={handleNewChat}
            onPickPrompt={handleSend}
            onEditPrompt={(p) => setDraft({ text: p.text, n: Date.now(), rewindFrom: p.taskId })}
            onRerun={(p) => handleSend(p.text, p.taskId)}
            leavingFrom={leavingFrom}
          />
          <ChatPanel
            onSend={handleSend}
            onStop={handleStop}
            onPause={handlePause}
            isRunning={isRunning}
            isPaused={isPaused}
            isConnected={isConnected}
            agentEnabled={agentEnabled}
            hasKey={hasKey}
            history={history}
            onOpenSettings={() => setIsSettingsOpen(true)}
            permission={permissionReq}
            onRespondPermission={handleRespondPermission}
            model={settings?.model_name ?? 'z-ai/glm-5.3-flash'}
            provider={settings?.provider ?? 'aicredits'}
            onModelChange={handleModelChange}
            effort={settings?.reasoning_effort ?? ''}
            onEffortChange={handleEffortChange}
            draft={draft}
          />
          </div>
        </section>
      </main>

      <SettingsModal
        isOpen={isSettingsOpen}
        localIp={localIp}
        onClose={() => setIsSettingsOpen(false)}
        onSaved={loadInitialData}
      />
    </div>
  );
}

export default App;
