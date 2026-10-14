import React, { useEffect, useMemo, useRef, useState } from 'react';
import { Check, ChevronRight, Minus, PanelLeft, SquarePen, X } from 'lucide-react';
import { AgentActionLog } from '../lib/types';
import { tauriApi } from '../lib/tauri';
import { MatsumotoCube } from './Matsumoto';
import { Markdown } from './Markdown';
import { MessageActions, MessageActionsContext, PromptRef } from './MessageActions';

interface ActionLogProps {
  logs: AgentActionLog[];
  isRunning: boolean;
  isPaused: boolean;
  canRun: boolean;
  title: string;
  chatsOpen: boolean;
  onToggleChats: () => void;
  onNewChat: () => void;
  onPickPrompt: (prompt: string) => void;
  onEditPrompt: (prompt: PromptRef) => void;
  /** Resend or retry an earlier message, replacing it and what follows. */
  onRerun: (prompt: PromptRef) => void;
  /** The message being replaced right now; it and everything after fold away. */
  leavingFrom: string | null;
}

type Speaker = 'you' | 'vivy' | 'matsumoto' | 'none';
type ToolStatus = 'running' | 'ok' | 'fail' | 'cancelled';

type Entry =
  | { kind: 'task'; id: string; text: string }
  | { kind: 'thought' | 'reply'; id: string; text: string; ts: number; prompt?: PromptRef }
  | { kind: 'reasoning'; id: string; text: string; ms?: number; ts: number }
  | { kind: 'tool'; id: string; tool: string; args: Record<string, any>; status: ToolStatus; result?: string; ms?: number }
  | { kind: 'done' | 'error' | 'stopped'; id: string; text: string; ts: number; prompt?: PromptRef }
  | { kind: 'compacted'; id: string; text: string };

interface Live {
  reasoning: string;
  content: string;
  ended: boolean;
}

/** Everyday things to try. A few are picked for each new chat. */
const EVERYDAY = [
  'Open YouTube and search for lo-fi music',
  'Turn on Wi-Fi from quick settings',
  'What is on my screen right now?',
  'Check how much battery my phone has left',
  "Search the web for tomorrow's weather",
  'Turn the volume down a little',
  'Open the camera',
  'Show me my latest notifications',
];

/** Little nods to Vivy and her cube-shaped partner. One shows up per new chat. */
const EASTER_EGGS = [
  'Who are you, and who is Matsumoto?',
  'Play a song that would make everyone happy',
  'Set a reminder for 100 years from now',
  'Matsumoto, keep it short this time and open the calculator',
  'Vivy, would you sing on a stage if I found you one?',
  'Matsumoto, what does the future look like from where you came from?',
];

function pickSuggestions(): string[] {
  const shuffled = [...EVERYDAY].sort(() => Math.random() - 0.5);
  const egg = EASTER_EGGS[Math.floor(Math.random() * EASTER_EGGS.length)];
  return [...shuffled.slice(0, 3), egg];
}

function summarizeArgs(tool: string, a: Record<string, any>): string {
  const q = (s: unknown) => (s ? `“${s}”` : '');
  const n = (v: unknown) => (typeof v === 'number' ? v.toFixed(1) : String(v ?? '?'));
  switch (tool) {
    case 'tap':
      return `${n(a.x)}, ${n(a.y)} ${a.element_label ? q(a.element_label) : ''}`;
    case 'long_press':
      return `${n(a.x)}, ${n(a.y)}`;
    case 'tap_element':
    case 'type_text':
      return q(a.text);
    case 'open_app':
      return a.app_name || a.package_name || '';
    case 'swipe':
      return `${n(a.x1)}, ${n(a.y1)} → ${n(a.x2)}, ${n(a.y2)}`;
    case 'scroll':
      return String(a.direction ?? '').toLowerCase();
    case 'press_key':
      return String(a.key ?? '');
    case 'wait':
      return `${a.seconds}s`;
    case 'web_search':
      return q(a.query);
    case 'inspect_screen':
      return [a.target && q(a.target), a.visual && 'with screenshot'].filter(Boolean).join(' ');
    case 'ask_permission':
      return `${a.category}: ${a.reason ?? ''}`;
    default:
      return Object.values(a).map(String).join(', ');
  }
}

/** What Matsumoto is doing, in words, for the live status line. */
function describeTool(tool: string, a: Record<string, any>): string {
  switch (tool) {
    case 'inspect_screen':
      return 'reading the screen';
    case 'tap':
    case 'tap_element':
      return `tapping ${a.text ? `“${a.text}”` : a.element_label ? `“${a.element_label}”` : 'the screen'}`;
    case 'long_press':
      return 'pressing and holding';
    case 'swipe':
      return 'swiping';
    case 'scroll':
      return `scrolling ${String(a.direction ?? 'down').toLowerCase()}`;
    case 'type_text':
      return 'typing';
    case 'press_key':
      return `pressing ${String(a.key ?? '').toLowerCase()}`;
    case 'open_app':
      return `opening ${a.app_name || a.package_name || 'an app'}`;
    case 'wait':
      return 'waiting for the phone';
    case 'web_search':
      return 'searching the web';
    case 'ask_permission':
      return 'waiting for your OK';
    default:
      return `running ${tool}`;
  }
}

function buildEntries(logs: AgentActionLog[]): Entry[] {
  const entries: Entry[] = [];
  let prompt: PromptRef | undefined; // the user message the following replies answer
  const toolIndex = new Map<string, number>();
  // Text that already streamed in live isn't typed out again (ts 0 = not fresh).
  const revealAt = (log: AgentActionLog) => (log.details?.live ? 0 : log.timestamp);

  for (const log of logs) {
    const d = log.details ?? {};
    switch (log.action_type) {
      case 'task':
        prompt = { text: log.description, taskId: log.id };
        entries.push({ kind: 'task', id: log.id, text: log.description });
        break;
      case 'thought':
        entries.push({ kind: 'thought', id: log.id, text: log.description, ts: revealAt(log), prompt });
        break;
      case 'reasoning':
        entries.push({ kind: 'reasoning', id: log.id, text: log.description, ms: d.ms, ts: revealAt(log) });
        break;
      case 'tool_call':
        if (d.tool === 'finish_task') break; // the summary shows as the outcome
        toolIndex.set(d.call_id, entries.length);
        entries.push({ kind: 'tool', id: log.id, tool: d.tool ?? log.description, args: d.arguments ?? {}, status: 'running' });
        break;
      case 'tool_result': {
        const idx = toolIndex.get(d.call_id);
        const entry = idx !== undefined ? entries[idx] : undefined;
        if (entry && entry.kind === 'tool') {
          entries[idx!] = { ...entry, status: d.ok ? 'ok' : 'fail', result: log.description, ms: d.ms };
        }
        break;
      }
      case 'compacted':
        entries.push({ kind: 'compacted', id: log.id, text: log.description });
        break;
      case 'finished':
      case 'error':
      case 'stopped':
        // Anything still in flight when the run ends never got a result.
        entries.forEach((en, i) => {
          if (en.kind === 'tool' && en.status === 'running') entries[i] = { ...en, status: 'cancelled' };
        });
        entries.push({
          kind: log.action_type === 'finished' ? (d.reply ? 'reply' : 'done') : log.action_type,
          id: log.id,
          text: log.description,
          ts: revealAt(log),
          prompt,
        });
        break;
      // permission_request is shown by the ask_permission tool row and the dialog
    }
  }
  return entries;
}

function speakerOf(e: Entry): Speaker {
  switch (e.kind) {
    case 'task':
      return 'you';
    case 'thought':
    case 'reasoning':
    case 'reply':
    case 'done':
      return 'vivy';
    case 'tool':
      return 'matsumoto';
    default:
      return 'none';
  }
}

const SpeakerLabel: React.FC<{ who: Speaker; busy: boolean }> = ({ who, busy }) => {
  // Your messages are already marked by sitting on the right.
  if (who === 'none' || who === 'you') return null;
  return (
    <div className="speaker">
      {who === 'matsumoto' && <MatsumotoCube busy={busy} />}
      {who === 'vivy' ? 'Vivy' : 'Matsumoto'}
    </div>
  );
};

export const ActionLog: React.FC<ActionLogProps> = ({
  logs,
  isRunning,
  isPaused,
  canRun,
  title,
  chatsOpen,
  onToggleChats,
  onNewChat,
  onPickPrompt,
  onEditPrompt,
  onRerun,
  leavingFrom,
}) => {
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const stick = useRef(true);
  const entries = useMemo(() => buildEntries(logs), [logs]);
  const [live, setLive] = useState<Live | null>(null);

  // Streamed tokens for the step in progress.
  useEffect(() => {
    const unlisten = tauriApi.onAgentStream((e) => {
      if (e.kind === 'start') setLive({ reasoning: '', content: '', ended: false });
      else if (e.kind === 'end') setLive((l) => (l ? { ...l, ended: true } : l));
      else
        setLive((l) => {
          const base = l ?? { reasoning: '', content: '', ended: false };
          return e.kind === 'reasoning'
            ? { ...base, reasoning: base.reasoning + e.text }
            : { ...base, content: base.content + e.text };
        });
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  // Once the finished step lands in the log, the live preview has done its job.
  useEffect(() => {
    setLive((l) => (l && l.ended ? null : l));
  }, [logs.length]);
  useEffect(() => {
    if (!isRunning) setLive(null);
  }, [isRunning]);

  // Follow the bottom unless the user scrolled up to read something.
  useEffect(() => {
    const el = scrollRef.current;
    if (el && stick.current) el.scrollTop = el.scrollHeight;
  }, [entries, live, isRunning, isPaused]);

  const onScroll = () => {
    const el = scrollRef.current;
    if (el) stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 60;
  };

  const last = entries[entries.length - 1];
  const lastLogAt = logs.length ? logs[logs.length - 1].timestamp : Date.now();
  const runningTool = last && last.kind === 'tool' && last.status === 'running' ? last : null;
  const showLive = Boolean(live && (live.reasoning || live.content));

  return (
    <>
      <div className="feed-head">
        <button
          className={`icon-btn ${chatsOpen ? 'on' : ''}`}
          onClick={onToggleChats}
          title={chatsOpen ? 'Hide chats' : 'Show chats'}
          aria-label="Toggle chats"
        >
          <PanelLeft size={15} />
        </button>
        <span className="feed-title" title={title}>
          {title}
        </span>
        <button
          className="icon-btn"
          onClick={onNewChat}
          disabled={isRunning}
          title={isRunning ? 'Finish or stop the current task first' : 'New chat'}
          aria-label="New chat"
        >
          <SquarePen size={15} />
        </button>
      </div>

      <MessageActionsContext.Provider value={{ canRun: canRun && !leavingFrom, onEdit: onEditPrompt, onResend: onRerun }}>
      <div className="feed" ref={scrollRef} onScroll={onScroll}>
        <div className="feed-inner">
          {entries.length === 0 ? (
            <div className="empty">
              <div className="empty-cube">
                <MatsumotoCube size={28} />
              </div>
              <h3>What should Vivy do?</h3>
              <p>
                Ask a question or describe something to do on the phone. Vivy plans it, Matsumoto taps through
                it, and you can watch it on the phone screen.
              </p>
              <Suggestions canRun={canRun} onPick={onPickPrompt} />
            </div>
          ) : (
            <Blocks entries={entries} leavingFrom={leavingFrom} />
          )}

          {isRunning && showLive && (
            <>
              {speakerOf(last ?? { kind: 'task', id: '', text: '' }) !== 'vivy' && <SpeakerLabel who="vivy" busy={false} />}
              {live!.reasoning && <LiveReasoning text={live!.reasoning} active={!live!.content} />}
              {live!.content && <Markdown text={live!.content} className="thought streaming" />}
            </>
          )}

          {isRunning && isPaused && (
            <div className="status-line paused">
              <span className="pause-bars" />
              Paused. Use the phone yourself if you like, then resume.
            </div>
          )}

          {isRunning && !isPaused && runningTool && (
            <div className="status-line">
              <span className="spinner" />
              <span className="shimmer">Matsumoto is {describeTool(runningTool.tool, runningTool.args)}</span>
            </div>
          )}

          {isRunning && !isPaused && !runningTool && !showLive && (
            <div className="status-line">
              <span className="orbit" />
              <span className="shimmer">Vivy is thinking</span>
              <ThinkingTimer since={lastLogAt} />
            </div>
          )}
        </div>
      </div>
      </MessageActionsContext.Provider>
    </>
  );
};

/** Reasoning tokens while they stream: a short, fading window onto the tail. */
const LiveReasoning: React.FC<{ text: string; active: boolean }> = ({ text, active }) => {
  const ref = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    if (ref.current) ref.current.scrollTop = ref.current.scrollHeight;
  }, [text]);
  return (
    <div className="reasoning live">
      <div className="reasoning-head">
        <span className={active ? 'shimmer' : ''}>{active ? 'Thinking' : 'Thought'}</span>
      </div>
      <div className="reasoning-tail" ref={ref}>
        {text}
      </div>
    </div>
  );
};

const EntryView: React.FC<{ entry: Entry }> = ({ entry: e }) => {
  switch (e.kind) {
    case 'task':
      return (
        <div className="msg msg-user">
          <div className="task">{e.text}</div>
          <MessageActions copyText={e.text} prompt={{ text: e.text, taskId: e.id }} />
        </div>
      );
    case 'thought':
    case 'reply':
      return (
        <div className="msg">
          <RevealText className="thought appear" text={e.text} ts={e.ts} />
          {e.kind === 'reply' && <MessageActions copyText={e.text} prompt={e.prompt} resendLabel="Retry" />}
        </div>
      );
    case 'reasoning':
      return <ReasoningEntry text={e.text} ms={e.ms} ts={e.ts} />;
    case 'tool':
      return (
        <details className={`tool appear ${e.status}`} open={e.status === 'fail' ? true : undefined}>
          <summary>
            <span className={`status ${e.status}`}>
              {e.status === 'ok' ? (
                <Check size={13} />
              ) : e.status === 'fail' ? (
                <X size={13} />
              ) : e.status === 'cancelled' ? (
                <Minus size={13} />
              ) : (
                <span className="spinner" />
              )}
            </span>
            <span className="name">{e.tool}</span>
            <span className="args">{summarizeArgs(e.tool, e.args)}</span>
            <span className="ms">{e.ms !== undefined ? `${(e.ms / 1000).toFixed(1)}s` : ''}</span>
          </summary>
          {e.result && <pre>{e.result}</pre>}
        </details>
      );
    case 'done':
      return (
        <div className="msg">
          <div className="outcome done appear">
            <Check size={15} className="outcome-icon" />
            <RevealText text={e.text} ts={e.ts} />
          </div>
          <MessageActions copyText={e.text} prompt={e.prompt} resendLabel="Retry" />
        </div>
      );
    case 'error':
      return (
        <div className="outcome error appear">
          <X size={15} className="outcome-icon" />
          <span>{e.text}</span>
        </div>
      );
    case 'compacted':
      return (
        <div className="compacted" title={e.text}>
          <span>Memory compacted</span>
        </div>
      );
    case 'stopped':
      return (
        <div className="outcome stopped appear">
          <Minus size={15} className="outcome-icon" />
          <span>Stopped. Nothing else will be sent to the phone.</span>
        </div>
      );
  }
};

/** Only entries that just arrived are animated; reopening old activity shows it as is. */
const FRESH_MS = 4000;

/**
 * Types text out quickly. Some providers deliver a whole reply in one burst
 * after the model finishes (aicredits' website host did), so it would
 * otherwise appear all at once. Replies that really streamed skip this. Long text speeds up so a reveal never takes long.
 */
function useReveal(text: string, ts: number): [string, boolean] {
  const [animate] = useState(() => Date.now() - ts < FRESH_MS);
  const [shown, setShown] = useState(animate ? 0 : text.length);

  useEffect(() => {
    if (!animate) {
      setShown(text.length);
      return;
    }
    const duration = Math.min(1400, Math.max(300, text.length * 3));
    // A timer rather than requestAnimationFrame: frames pause while the window
    // is hidden or covered, which would leave text half-revealed.
    const start = Date.now();
    const id = window.setInterval(() => {
      const t = Math.min(1, (Date.now() - start) / duration);
      setShown(Math.ceil(text.length * t));
      if (t >= 1) window.clearInterval(id);
    }, 30);
    return () => window.clearInterval(id);
  }, [animate, text]);

  return [text.slice(0, shown), shown < text.length];
}

const RevealText: React.FC<{ text: string; ts: number; className?: string }> = ({ text, ts, className }) => {
  const [visible] = useReveal(text, ts);
  return <Markdown text={visible} className={className} />;
};

/** Shows the thinking typing out, then folds it away into "Thought for Ns". */
const ReasoningEntry: React.FC<{ text: string; ms?: number; ts: number }> = ({ text, ms, ts }) => {
  const [visible, typing] = useReveal(text, ts);
  const tailRef = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    if (tailRef.current) tailRef.current.scrollTop = tailRef.current.scrollHeight;
  }, [visible]);

  if (typing) {
    return (
      <div className="reasoning live">
        <div className="reasoning-head">
          <span className="shimmer">Thinking</span>
        </div>
        <div className="reasoning-tail" ref={tailRef}>
          {visible}
        </div>
      </div>
    );
  }
  return (
    <details className="reasoning">
      <summary>
        <ChevronRight size={13} className="chev" />
        Thought{ms !== undefined ? ` for ${(ms / 1000).toFixed(1)}s` : ''}
      </summary>
      <div className="reasoning-body">{text}</div>
    </details>
  );
};

/** "· 7s" next to "Vivy is thinking", so a long silent wait doesn't look frozen. */
const ThinkingTimer: React.FC<{ since: number }> = ({ since }) => {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), 500);
    return () => window.clearInterval(id);
  }, []);
  const secs = Math.floor((now - since) / 1000);
  return secs >= 2 ? <span className="timer">· {secs}s</span> : null;
};

/**
 * The conversation. When a message is resent, it and everything after it fold
 * away while Matsumoto tidies up, then the fresh run appears in its place.
 */
const Blocks: React.FC<{ entries: Entry[]; leavingFrom: string | null }> = ({ entries, leavingFrom }) => {
  const blocks = groupEntries(entries);
  const nodes = blocks.map((block, i) => {
    if (block.kind === 'tools') {
      return <ToolGroup key={block.items[0].id} items={block.items} />;
    }
    const e = block.entry;
    const who = speakerOf(e);
    const prev = blocks[i - 1];
    const prevWho = !prev ? null : prev.kind === 'tools' ? 'matsumoto' : speakerOf(prev.entry);
    return (
      <React.Fragment key={e.id}>
        {who !== 'none' && who !== prevWho && <SpeakerLabel who={who} busy={false} />}
        <EntryView entry={e} />
      </React.Fragment>
    );
  });

  const cut = leavingFrom
    ? blocks.findIndex((b) => b.kind === 'entry' && b.entry.kind === 'task' && b.entry.id === leavingFrom)
    : -1;
  if (cut < 0) return <>{nodes}</>;
  return (
    <>
      {nodes.slice(0, cut)}
      <div className="rewind-out" aria-hidden>
        <div className="rewind-inner">{nodes.slice(cut)}</div>
      </div>
      <div className="status-line rewind-sweep">
        <MatsumotoCube busy />
        <span className="shimmer">Matsumoto is tidying up</span>
      </div>
    </>
  );
};

/** Starter prompts for an empty chat, freshly picked each time one opens. */
const Suggestions: React.FC<{ canRun: boolean; onPick: (prompt: string) => void }> = ({ canRun, onPick }) => {
  const [items] = useState(pickSuggestions);
  return (
    <div className="examples">
      {items.map((ex) => (
        <button key={ex} disabled={!canRun} onClick={() => onPick(ex)}>
          {ex}
        </button>
      ))}
    </div>
  );
};

type ToolEntry = Extract<Entry, { kind: 'tool' }>;
type Block = { kind: 'tools'; items: ToolEntry[] } | { kind: 'entry'; entry: Entry };

/** Consecutive tool calls become one collapsible Matsumoto block. */
function groupEntries(entries: Entry[]): Block[] {
  const blocks: Block[] = [];
  for (const e of entries) {
    const prev = blocks[blocks.length - 1];
    if (e.kind === 'tool') {
      if (prev && prev.kind === 'tools') prev.items.push(e);
      else blocks.push({ kind: 'tools', items: [e] });
    } else {
      blocks.push({ kind: 'entry', entry: e });
    }
  }
  return blocks;
}

/** What a finished tool call did, in a couple of words, for the folded summary. */
function pastTense(tool: string, a: Record<string, any>): string {
  switch (tool) {
    case 'tap':
    case 'tap_element':
      return 'tapped';
    case 'long_press':
      return 'held';
    case 'swipe':
      return 'swiped';
    case 'scroll':
      return 'scrolled';
    case 'type_text':
      return 'typed';
    case 'press_key':
      return `pressed ${String(a.key ?? 'a key').toLowerCase()}`;
    case 'open_app':
      return `opened ${a.app_name || a.package_name || 'an app'}`;
    case 'wait':
      return 'waited';
    case 'inspect_screen':
      return 'read the screen';
    case 'web_search':
      return 'searched the web';
    case 'ask_permission':
      return 'asked you';
    default:
      return tool;
  }
}

/**
 * Matsumoto's actions between two of Vivy's messages. Open while he's working,
 * folded into a one-line summary once he's done; click the header to toggle.
 */
const ToolGroup: React.FC<{ items: ToolEntry[] }> = ({ items }) => {
  const running = items.some((t) => t.status === 'running');
  const [manual, setManual] = useState<boolean | null>(null);
  const open = manual ?? running;

  const failed = items.filter((t) => t.status === 'fail').length;
  const totalMs = items.reduce((sum, t) => sum + (t.ms ?? 0), 0);
  const verbs = [...new Set(items.map((t) => pastTense(t.tool, t.args)))];
  const summary = verbs.slice(0, 4).join(', ') + (verbs.length > 4 ? ` +${verbs.length - 4} more` : '');
  const current = items.find((t) => t.status === 'running');

  return (
    <div className={`tool-group ${open ? 'open' : ''}`}>
      <button className="speaker group-head" onClick={() => setManual(!open)} aria-expanded={open}>
        <MatsumotoCube busy={running} />
        <span className="group-name">Matsumoto</span>
        {!open && (
          <span className="group-summary">
            {current ? describeTool(current.tool, current.args) : summary}
          </span>
        )}
        {!open && (
          <span className="group-meta">
            {failed > 0 && <span className="group-failed">{failed} failed · </span>}
            {items.length} {items.length === 1 ? 'step' : 'steps'}
            {totalMs > 0 && ` · ${(totalMs / 1000).toFixed(1)}s`}
          </span>
        )}
        <ChevronRight size={13} className="chev" />
      </button>
      {open && items.map((t) => <EntryView key={t.id} entry={t} />)}
    </div>
  );
};
