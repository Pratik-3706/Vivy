import React, { useEffect, useRef, useState } from 'react';
import { Check, ChevronLeft, Circle, Copy, Lock, Square } from 'lucide-react';
import { DeviceState } from '../lib/types';
import { tauriApi } from '../lib/tauri';
import { formatCountdown, usePairing } from '../lib/pairing';

interface PhoneScreenProps {
  device: DeviceState | null;
  connectionUrl: string;
  localIp: string;
  agentActive: boolean;
  phoneError: string | null;
}

interface TapRing {
  id: number;
  x: number;
  y: number;
}

/** Where the agent touched, in percent of the screen. */
interface AgentMark {
  id: number;
  kind: 'tap' | 'hold' | 'swipe';
  x: number;
  y: number;
  x2?: number;
  y2?: number;
  label?: string;
}

const MARK_MS = 1600;

/** Wheel events arrive in bursts; one swipe per burst is plenty. */
const WHEEL_COOLDOWN_MS = 350;

export const PhoneScreen: React.FC<PhoneScreenProps> = ({
  device,
  connectionUrl,
  localIp,
  agentActive,
  phoneError,
}) => {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const [hasFrame, setHasFrame] = useState(false);
  const [aspect, setAspect] = useState<number | null>(null);
  const [fps, setFps] = useState(0);
  const [rings, setRings] = useState<TapRing[]>([]);
  const [marks, setMarks] = useState<AgentMark[]>([]);
  const [copied, setCopied] = useState(false);
  const [typed, setTyped] = useState('');
  const dragStart = useRef<{ x: number; y: number } | null>(null);
  const lastWheel = useRef(0);

  const connected = Boolean(device?.connected);

  // Frames are drawn here rather than lifted into App state, so 12 fps of
  // images don't re-render the whole window.
  useEffect(() => {
    if (!connected) {
      setHasFrame(false);
      setFps(0);
      return;
    }

    let frames = 0;
    let decoding = false;
    let pending: string | null = null;
    let disposed = false;

    const draw = (b64: string) => {
      decoding = true;
      const img = new Image();
      img.onload = () => {
        decoding = false;
        const canvas = canvasRef.current;
        if (!disposed && canvas) {
          if (canvas.width !== img.naturalWidth || canvas.height !== img.naturalHeight) {
            canvas.width = img.naturalWidth;
            canvas.height = img.naturalHeight;
            setAspect(img.naturalWidth / img.naturalHeight);
          }
          canvas.getContext('2d')?.drawImage(img, 0, 0);
          setHasFrame(true);
          frames += 1;
        }
        // Skip straight to the newest frame if more arrived while decoding.
        if (pending) {
          const next = pending;
          pending = null;
          draw(next);
        }
      };
      img.onerror = () => {
        decoding = false;
      };
      img.src = b64.startsWith('data:') ? b64 : `data:image/jpeg;base64,${b64}`;
    };

    const onFrame = (b64: string) => {
      if (decoding) pending = b64;
      else draw(b64);
    };

    const timer = window.setInterval(() => {
      setFps(frames);
      frames = 0;
    }, 1000);

    tauriApi.getLatestFrame().then((f) => f && onFrame(f)).catch(() => {});
    const unlisten = tauriApi.onPhoneFrame((p) => onFrame(p.frame));

    return () => {
      disposed = true;
      window.clearInterval(timer);
      unlisten.then((fn) => fn());
    };
  }, [connected]);

  // Show what the agent touches, so you can follow along on the mirror.
  useEffect(() => {
    const addMark = (m: Omit<AgentMark, 'id'>) => {
      const id = Date.now() + Math.random();
      setMarks((prev) => [...prev, { ...m, id }]);
      window.setTimeout(() => setMarks((prev) => prev.filter((x) => x.id !== id)), MARK_MS);
    };
    const num = (v: unknown) => (typeof v === 'number' ? v : Number(v));

    const unlisten = tauriApi.onAgentAction((log) => {
      const d = log.details ?? {};
      const a = d.arguments ?? {};
      if (log.action_type === 'tool_call') {
        if (d.tool === 'tap') addMark({ kind: 'tap', x: num(a.x), y: num(a.y), label: a.element_label });
        else if (d.tool === 'long_press') addMark({ kind: 'hold', x: num(a.x), y: num(a.y) });
        else if (d.tool === 'swipe')
          addMark({ kind: 'swipe', x: num(a.x1), y: num(a.y1), x2: num(a.x2), y2: num(a.y2) });
        else if (d.tool === 'scroll') {
          const dir = String(a.direction ?? 'DOWN').toUpperCase();
          const v = { DOWN: [50, 72, 50, 30], UP: [50, 30, 50, 72], LEFT: [20, 50, 80, 50], RIGHT: [80, 50, 20, 50] }[dir] ?? [50, 72, 50, 30];
          addMark({ kind: 'swipe', x: v[0], y: v[1], x2: v[2], y2: v[3] });
        }
      } else if (log.action_type === 'tool_result' && d.tool === 'tap_element' && d.ok) {
        // The phone reports where the matched element was: Tapped "Search" at (84.2, 6.1)
        const m = /"(.*)" at \(([\d.]+), ([\d.]+)\)/.exec(log.description);
        if (m) addMark({ kind: 'tap', x: Number(m[2]), y: Number(m[3]), label: m[1] });
      }
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  const toPercent = (e: React.MouseEvent) => {
    const rect = canvasRef.current!.getBoundingClientRect();
    return {
      x: Math.min(100, Math.max(0, ((e.clientX - rect.left) / rect.width) * 100)),
      y: Math.min(100, Math.max(0, ((e.clientY - rect.top) / rect.height) * 100)),
      px: e.clientX - rect.left,
      py: e.clientY - rect.top,
    };
  };

  const onMouseDown = (e: React.MouseEvent<HTMLCanvasElement>) => {
    if (e.button !== 0) return;
    const p = toPercent(e);
    dragStart.current = { x: p.x, y: p.y };
    const id = Date.now() + Math.random();
    setRings((r) => [...r, { id, x: p.px, y: p.py }]);
    window.setTimeout(() => setRings((r) => r.filter((m) => m.id !== id)), 450);
  };

  const onMouseUp = (e: React.MouseEvent<HTMLCanvasElement>) => {
    const start = dragStart.current;
    dragStart.current = null;
    if (!start) return;
    const end = toPercent(e);
    const moved = Math.hypot(end.x - start.x, end.y - start.y);
    const action =
      moved < 2
        ? tauriApi.manualTouch(start.x, start.y)
        : tauriApi.manualSwipe(start.x, start.y, end.x, end.y, 280);
    action.catch((err) => console.error('Touch error:', err));
  };

  const onWheel = (e: React.WheelEvent<HTMLCanvasElement>) => {
    const now = Date.now();
    if (now - lastWheel.current < WHEEL_COOLDOWN_MS) return;
    lastWheel.current = now;
    const p = toPercent(e);
    const dy = e.deltaY > 0 ? -25 : 25;
    const y2 = Math.min(95, Math.max(5, p.y + dy));
    tauriApi.manualSwipe(p.x, p.y, p.x, y2, 220).catch(() => {});
  };

  const sendKey = (key: string) => tauriApi.manualKey(key).catch((err) => console.error(err));

  const sendText = (e: React.FormEvent) => {
    e.preventDefault();
    if (!typed) return;
    tauriApi.manualType(typed).catch((err) => console.error(err));
    setTyped('');
  };

  const copyAddress = () => {
    navigator.clipboard.writeText(localIp).then(() => {
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    });
  };

  if (!connected) {
    return (
      <section className="device-pane">
        <div className="screen-area">
          <PairingPanel connectionUrl={connectionUrl} copied={copied} onCopy={copyAddress} phoneError={phoneError} />
        </div>
      </section>
    );
  }

  const ratio = aspect ?? (device ? device.screen_width / device.screen_height : 9 / 19.5);

  return (
    <section className="device-pane">
      <div className="screen-area">
        <div
          className={`screen ${agentActive ? 'agent-active' : ''}`}
          style={{ aspectRatio: String(ratio) }}
          title={agentActive ? 'Vivy is controlling the phone. Press Esc to stop.' : undefined}
        >
          <canvas
            ref={canvasRef}
            onMouseDown={onMouseDown}
            onMouseUp={onMouseUp}
            onMouseLeave={() => (dragStart.current = null)}
            onWheel={onWheel}
            onContextMenu={(e) => {
              e.preventDefault();
              sendKey('BACK');
            }}
          />
          {!hasFrame && (
            <div className="screen-overlay">
              <strong>Waiting for the screen</strong>
              <span>
                {phoneError ??
                  'The phone is connected but hasn’t sent a picture yet. Check that screen sharing is allowed in the Vivy app.'}
              </span>
            </div>
          )}
          {rings.map((r) => (
            <span key={r.id} className="tap-ring" style={{ left: r.x, top: r.y }} />
          ))}
          {marks.some((m) => m.kind === 'swipe') && (
            <svg className="agent-swipes">
              {marks
                .filter((m) => m.kind === 'swipe')
                .map((m) => (
                  <line key={m.id} x1={`${m.x}%`} y1={`${m.y}%`} x2={`${m.x2}%`} y2={`${m.y2}%`} pathLength={1} />
                ))}
            </svg>
          )}
          {marks
            .filter((m) => m.kind !== 'swipe')
            .map((m) => (
              <span
                key={m.id}
                className={`agent-mark ${m.kind}`}
                style={{ left: `${m.x}%`, top: `${m.y}%` }}
              >
                {m.label && <span className="agent-mark-label">{m.label}</span>}
              </span>
            ))}
        </div>
      </div>

      <div className="device-controls">
        {device?.accessibility === false && (
          <div className="banner">
            Accessibility is off on the phone, so Vivy can&rsquo;t tap, type or read the screen. Turn it on in
            Settings &rsaquo; Accessibility &rsaquo; Vivy.
          </div>
        )}
        {device?.screen_share === false && (
          <div className="banner">Screen sharing is off on the phone. Allow it in the Vivy app to see the screen.</div>
        )}
        <div className="nav-keys">
          <button className="icon-btn" onClick={() => sendKey('BACK')} title="Back (or right-click the screen)">
            <ChevronLeft size={18} />
          </button>
          <button className="icon-btn" onClick={() => sendKey('HOME')} title="Home">
            <Circle size={14} />
          </button>
          <button className="icon-btn" onClick={() => sendKey('RECENTS')} title="Recent apps">
            <Square size={13} />
          </button>
        </div>

        <form className="inline-input" onSubmit={sendText}>
          <input
            className="field"
            value={typed}
            onChange={(e) => setTyped(e.target.value)}
            placeholder="Type into the focused field on the phone"
          />
          <button className="btn btn-ghost" type="submit" disabled={!typed}>
            Send
          </button>
        </form>

        <div className="stream-meta">
          <span>
            <span className={`dot ${hasFrame ? 'ok' : 'warn'}`} style={{ display: 'inline-block', marginRight: 6 }} />
            {hasFrame ? `Live · ${fps} fps` : 'No picture'}
          </span>
          <span title="Click to tap, drag to swipe, mouse wheel to scroll, right-click for Back">Mouse controls the phone</span>
        </div>
      </div>
    </section>
  );
};

/** How to connect: the PC address, plus the pairing code until a phone is paired. */
const PairingPanel: React.FC<{
  connectionUrl: string;
  copied: boolean;
  onCopy: () => void;
  phoneError: string | null;
}> = ({ connectionUrl, copied, onCopy, phoneError }) => {
  const { pairing, secondsLeft, newCode, cancel, forget } = usePairing();
  const [confirmForget, setConfirmForget] = useState(false);
  const paired = pairing?.paired ?? null;
  const code = pairing?.code ?? null;

  return (
    <div className="pairing">
      <div>
        <h2>{paired && !code ? 'Waiting for your phone' : 'Connect your phone'}</h2>
        <p>
          {paired && !code
            ? `Paired with ${paired.device_name}. Open Vivy on the phone and tap Connect.`
            : 'Open Vivy on the phone and enter this address and pairing code.'}
        </p>
      </div>

      <div className="address">
        <span>{connectionUrl}</span>
        <button onClick={onCopy} title="Copy IP address">
          {copied ? <Check size={13} /> : <Copy size={13} />}
          {copied ? 'Copied' : 'Copy'}
        </button>
      </div>

      {code && (
        <div className="pair-code">
          <div className="pair-code-label">Pairing code</div>
          <div className="pair-code-value">{code}</div>
          <div className="pair-code-foot">
            <span>Expires in {formatCountdown(secondsLeft)}</span>
            <span className="pair-code-actions">
              <button onClick={newCode}>New code</button>
              {paired && <button onClick={cancel}>Cancel</button>}
            </span>
          </div>
        </div>
      )}

      {!paired && (
        <ol className="steps">
          <li>Turn on Accessibility for Vivy.</li>
          <li>Allow screen sharing.</li>
          <li>Enter the address and code above, then tap Pair.</li>
        </ol>
      )}

      <p className="hint secure-note">
        <Lock size={12} />
        {paired
          ? 'Only your paired phone can connect, and everything between them is encrypted.'
          : 'The code pairs this PC with one phone. After that, only that phone can connect and everything is encrypted.'}
      </p>

      {paired && !code && (
        <div className="pair-manage">
          <button onClick={newCode}>Pair a different phone</button>
          {confirmForget ? (
            <button
              className="danger"
              onClick={() => {
                setConfirmForget(false);
                forget();
              }}
            >
              Forget {paired.device_name}?
            </button>
          ) : (
            <button onClick={() => setConfirmForget(true)}>Forget phone</button>
          )}
        </div>
      )}

      <p className="hint">
        Same Wi-Fi network required. Over USB, run <code>adb reverse tcp:8765 tcp:8765</code> and use{' '}
        <code>127.0.0.1</code>.
      </p>
      {phoneError && <div className="banner">{phoneError}</div>}
    </div>
  );
};
