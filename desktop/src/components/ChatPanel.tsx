import React, { useEffect, useRef, useState } from 'react';
import { ArrowUp, Pause, Pencil, Play, ShieldAlert, Square, X } from 'lucide-react';
import { PermissionRequest } from '../lib/types';
import { ModelPicker } from './ModelPicker';
import { EffortPicker } from './EffortPicker';

interface ChatPanelProps {
  /** `rewindFrom`: the earlier message this one replaces (after Edit). */
  onSend: (prompt: string, rewindFrom?: string) => void;
  onStop: () => void;
  onPause: (paused: boolean) => void;
  isRunning: boolean;
  isPaused: boolean;
  isConnected: boolean;
  agentEnabled: boolean;
  hasKey: boolean;
  history: string[];
  onOpenSettings: () => void;
  permission: PermissionRequest | null;
  onRespondPermission: (id: string, allowed: boolean) => void;
  model: string;
  provider: string;
  onModelChange: (id: string) => void;
  effort: string;
  onEffortChange: (id: string) => void;
  /** Text to load into the box (Edit on a message); `n` changes each time. */
  draft: { text: string; n: number; rewindFrom?: string } | null;
}

/** These are asked every time; the rest are remembered until the agent is switched off. */
const ALWAYS_ASK = new Set(['destructive', 'financial']);

export const ChatPanel: React.FC<ChatPanelProps> = ({
  onSend,
  onStop,
  onPause,
  isRunning,
  isPaused,
  isConnected,
  agentEnabled,
  hasKey,
  history,
  onOpenSettings,
  permission,
  onRespondPermission,
  model,
  provider,
  onModelChange,
  effort,
  onEffortChange,
  draft,
}) => {
  const [text, setText] = useState('');
  // -1 = editing a fresh prompt; 0.. = browsing history with the arrow keys
  const [historyPos, setHistoryPos] = useState(-1);
  // Set while editing an earlier message: sending replaces it.
  const [editing, setEditing] = useState<string | null>(null);
  const ref = useRef<HTMLTextAreaElement | null>(null);

  // Grow with the typed text up to the CSS max-height. Measured again when the
  // width changes; an empty box is always one line (a wrapping placeholder
  // must not size it, e.g. while the window is still laying out).
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const fit = () => {
      el.style.height = 'auto';
      if (!el.value) {
        el.style.overflowY = 'hidden';
        return;
      }
      el.style.height = `${el.scrollHeight}px`;
      el.style.overflowY = el.scrollHeight > 160 ? 'auto' : 'hidden';
    };
    fit();
    // Only width changes matter; reacting to our own height change would loop.
    let width = el.clientWidth;
    const observer = new ResizeObserver(() => {
      if (el.clientWidth !== width) {
        width = el.clientWidth;
        fit();
      }
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, [text]);

  useEffect(() => {
    if (!draft) return;
    setText(draft.text);
    setHistoryPos(-1);
    setEditing(draft.rewindFrom ?? null);
    const el = ref.current;
    if (el) {
      el.focus();
      requestAnimationFrame(() => el.setSelectionRange(el.value.length, el.value.length));
    }
  }, [draft]);

  const canSend = agentEnabled && !isRunning && text.trim().length > 0;

  const submit = () => {
    if (!canSend) return;
    if (!hasKey) {
      onOpenSettings();
      return;
    }
    onSend(text.trim(), editing ?? undefined);
    setText('');
    setHistoryPos(-1);
    setEditing(null);
  };

  const recall = (pos: number) => {
    setHistoryPos(pos);
    setText(pos < 0 ? '' : history[pos]);
  };

  const cancelEdit = () => {
    setEditing(null);
    setText('');
  };

  const onKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Escape' && editing && !isRunning) {
      e.preventDefault();
      cancelEdit();
      return;
    }
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      submit();
      return;
    }
    // Shell-style history, only when the caret is on the first/last line.
    const el = e.currentTarget;
    if (e.key === 'ArrowUp' && history.length && !el.value.slice(0, el.selectionStart).includes('\n')) {
      if (historyPos < history.length - 1 && (text === '' || historyPos >= 0)) {
        e.preventDefault();
        recall(historyPos + 1);
      }
    } else if (e.key === 'ArrowDown' && historyPos >= 0 && !el.value.slice(el.selectionEnd).includes('\n')) {
      e.preventDefault();
      recall(historyPos - 1);
    }
  };

  const hint = !agentEnabled
    ? 'The agent is switched off.'
    : isPaused
      ? 'Paused between steps. The phone is all yours.'
      : isRunning
        ? 'Working on it.'
        : !hasKey
          ? 'Add an API key in Settings first.'
          : !isConnected
            ? 'No phone connected. Vivy can still chat.'
            : '';

  // Enter allows, Esc denies, unless you're typing in the message box.
  useEffect(() => {
    if (!permission) return;
    const onKey = (e: KeyboardEvent) => {
      if (document.activeElement === ref.current) return;
      if (e.key === 'Enter') {
        e.preventDefault();
        onRespondPermission(permission.id, true);
      } else if (e.key === 'Escape') {
        e.preventDefault();
        onRespondPermission(permission.id, false);
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [permission, onRespondPermission]);

  return (
    <div className="composer">
      <div className="composer-inner">
        {permission && (
          <div className="permission-bar" role="alertdialog" aria-label={`Vivy needs your OK: ${permission.title}`}>
            <ShieldAlert size={14} className="permission-icon" />
            <span className="permission-title">{permission.title}</span>
            <span
              className="permission-reason"
              title={`${permission.reason}\n\n${
                ALWAYS_ASK.has(permission.category)
                  ? 'Asked every time for this kind of action.'
                  : 'Allowing covers this category until the agent is switched off.'
              }`}
            >
              {permission.reason}
            </span>
            <button className="perm-btn" onClick={() => onRespondPermission(permission.id, false)}>
              Deny
            </button>
            <button className="perm-btn allow" onClick={() => onRespondPermission(permission.id, true)}>
              Allow
            </button>
          </div>
        )}
        {editing && !permission && (
          <div className="edit-bar">
            <Pencil size={13} className="edit-icon" />
            <span className="edit-title">Editing a message</span>
            <span className="edit-note">Sending replaces it and everything after it.</span>
            <button className="icon-btn" onClick={cancelEdit} title="Cancel editing (Esc)" aria-label="Cancel editing">
              <X size={13} />
            </button>
          </div>
        )}
        <div className={`composer-box ${isRunning && !isPaused ? 'running' : ''}`}>
          <textarea
            ref={ref}
            rows={1}
            value={text}
            disabled={!agentEnabled}
            onChange={(e) => {
              setText(e.target.value);
              setHistoryPos(-1);
            }}
            onKeyDown={onKeyDown}
            placeholder={
              !agentEnabled
                ? 'Agent is off. Turn it on in the top bar to run tasks.'
                : isConnected
                  ? 'Ask Vivy something, or tell her what to do on the phone'
                  : 'Ask Vivy something. Connect a phone for phone tasks.'
            }
          />
          {isRunning ? (
            <div className="run-controls">
              <button
                className="btn btn-ghost"
                onClick={() => onPause(!isPaused)}
                title={isPaused ? 'Resume' : 'Pause after the current step'}
              >
                {isPaused ? <Play size={12} fill="currentColor" /> : <Pause size={12} fill="currentColor" />}
                {isPaused ? 'Resume' : 'Pause'}
              </button>
              <button className="btn btn-stop" onClick={onStop} title="Stop (Esc, or Ctrl+Shift+X from anywhere)">
                <Square size={11} fill="currentColor" />
                Stop
              </button>
            </div>
          ) : (
            <button className="btn btn-primary" onClick={submit} disabled={!canSend} title="Run (Enter)">
              <ArrowUp size={14} />
              Run
            </button>
          )}
        </div>
        <div className="composer-hint">
          <span className="hint-left">
            <span className="model-controls">
              <ModelPicker value={model} provider={provider} disabled={isRunning} onChange={onModelChange} />
              <EffortPicker value={effort} provider={provider} disabled={isRunning} onChange={onEffortChange} />
            </span>
            {hint && <span>{hint}</span>}
          </span>
          <span>
            {isRunning ? (
              <>
                <kbd>Esc</kbd> stop · <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>X</kbd> stop from anywhere
              </>
            ) : (
              <>
                <kbd>Enter</kbd> run · <kbd>Shift</kbd>+<kbd>Enter</kbd> new line · <kbd>↑</kbd> history
              </>
            )}
          </span>
        </div>
      </div>
    </div>
  );
};
