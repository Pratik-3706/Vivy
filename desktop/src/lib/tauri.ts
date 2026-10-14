import { invoke } from '@tauri-apps/api/core';
import { listen, UnlistenFn } from '@tauri-apps/api/event';
import {
  AgentActionLog,
  AgentStatus,
  ConnectionInfo,
  DeviceState,
  PermissionRequest,
  Session,
  SessionMeta,
  SettingsView,
  StreamEvent,
  SaveSettings,
  PairingView,
} from './types';

export const tauriApi = {
  getSettings: (): Promise<SettingsView> => invoke('get_settings'),

  saveSettings: (payload: SaveSettings): Promise<void> => invoke('save_settings', { payload }),

  getDeviceStatus: (): Promise<DeviceState> => invoke('get_device_status'),

  getLatestFrame: (): Promise<string | null> => invoke('get_latest_frame'),

  getConnectionInfo: (): Promise<ConnectionInfo> => invoke('get_connection_info'),

  /** `rewindFrom`: id of an earlier message to replace, with everything after it. */
  startAgentTask: (prompt: string, rewindFrom?: string): Promise<void> =>
    invoke('start_agent_task', { prompt, rewindFrom: rewindFrom ?? null }),

  stopAgentTask: (): Promise<void> => invoke('stop_agent_task'),

  listSessions: (): Promise<SessionMeta[]> => invoke('list_sessions'),

  getActiveSession: (): Promise<Session> => invoke('get_active_session'),

  newSession: (): Promise<Session> => invoke('new_session'),

  openSession: (id: string): Promise<Session> => invoke('open_session', { id }),

  deleteSession: (id: string): Promise<Session> => invoke('delete_session', { id }),

  renameSession: (id: string, title: string): Promise<void> => invoke('rename_session', { id, title }),

  getAgentStatus: (): Promise<AgentStatus> => invoke('get_agent_status'),

  setAgentEnabled: (enabled: boolean): Promise<boolean> => invoke('set_agent_enabled', { enabled }),

  setAgentPaused: (paused: boolean): Promise<boolean> => invoke('set_agent_paused', { paused }),

  respondToPermission: (requestId: string, allowed: boolean): Promise<boolean> =>
    invoke('respond_to_permission', { requestId, allowed }),

  manualTouch: (x: number, y: number): Promise<void> => invoke('manual_touch', { x, y }),

  manualSwipe: (
    x1: number,
    y1: number,
    x2: number,
    y2: number,
    durationMs?: number
  ): Promise<void> => invoke('manual_swipe', { x1, y1, x2, y2, durationMs }),

  manualKey: (key: string): Promise<void> => invoke('manual_key', { key }),

  manualType: (text: string): Promise<void> => invoke('manual_type', { text }),

  // Events
  onPhoneFrame: (cb: (payload: { frame: string }) => void): Promise<UnlistenFn> =>
    listen<{ frame: string }>('phone-frame', (e) => cb(e.payload)),

  onPhoneConnected: (cb: (device: DeviceState) => void): Promise<UnlistenFn> =>
    listen<DeviceState>('phone-connected', (e) => cb(e.payload)),

  onPhoneDisconnected: (cb: () => void): Promise<UnlistenFn> =>
    listen('phone-disconnected', () => cb()),

  onPhoneStatus: (cb: (device: DeviceState) => void): Promise<UnlistenFn> =>
    listen<DeviceState>('phone-status', (e) => cb(e.payload)),

  onPhoneError: (cb: (message: string) => void): Promise<UnlistenFn> =>
    listen<{ error: string }>('phone-error', (e) => cb(e.payload.error)),

  onAgentAction: (cb: (log: AgentActionLog) => void): Promise<UnlistenFn> =>
    listen<AgentActionLog>('agent-action', (e) => cb(e.payload)),

  onPermissionRequest: (cb: (req: PermissionRequest) => void): Promise<UnlistenFn> =>
    listen<PermissionRequest>('agent-permission-request', (e) => cb(e.payload)),

  onPermissionCancelled: (cb: () => void): Promise<UnlistenFn> =>
    listen('agent-permission-cancelled', () => cb()),

  onAgentComplete: (cb: (res: { summary: string }) => void): Promise<UnlistenFn> =>
    listen<{ summary: string }>('agent-complete', (e) => cb(e.payload)),

  onAgentError: (cb: (err: { error: string }) => void): Promise<UnlistenFn> =>
    listen<{ error: string }>('agent-error', (e) => cb(e.payload)),

  onAgentStopped: (cb: () => void): Promise<UnlistenFn> => listen('agent-stopped', () => cb()),

  onAgentStream: (cb: (e: StreamEvent) => void): Promise<UnlistenFn> =>
    listen<StreamEvent>('agent-stream', (e) => cb(e.payload)),

  onAgentPaused: (cb: (paused: boolean) => void): Promise<UnlistenFn> =>
    listen<boolean>('agent-paused', (e) => cb(e.payload)),

  onSessionsChanged: (cb: () => void): Promise<UnlistenFn> => listen('sessions-changed', () => cb()),

  onAgentEnabledChanged: (cb: (enabled: boolean) => void): Promise<UnlistenFn> =>
    listen<boolean>('agent-enabled-changed', (e) => cb(e.payload)),

  getPairing: (): Promise<PairingView> => invoke('get_pairing'),

  /** Show a fresh pairing code (to pair a different phone). */
  startPairing: (): Promise<PairingView> => invoke('start_pairing'),

  cancelPairing: (): Promise<PairingView> => invoke('cancel_pairing'),

  /** Unpair the phone and disconnect it. */
  forgetPhone: (): Promise<PairingView> => invoke('forget_phone'),

  onPairingChanged: (cb: (p: PairingView) => void): Promise<UnlistenFn> =>
    listen<PairingView>('pairing-changed', (e) => cb(e.payload)),
};

/** Subscribe to several events and get back one cleanup function. */
export function subscribeAll(subs: Promise<UnlistenFn>[]): () => void {
  let disposed = false;
  const fns: UnlistenFn[] = [];
  subs.forEach((p) =>
    p.then((fn) => {
      if (disposed) fn();
      else fns.push(fn);
    })
  );
  return () => {
    disposed = true;
    fns.forEach((fn) => fn());
  };
}
