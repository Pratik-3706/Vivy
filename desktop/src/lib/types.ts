export interface DeviceState {
  connected: boolean;
  device_name: string;
  brand: string;
  model: string;
  android_version: string;
  screen_width: number;
  screen_height: number;
  battery_level: number;
  is_charging: boolean;
  streaming: boolean;
  last_seen: number;
  /** null when the phone app is too old to report it */
  accessibility?: boolean | null;
  screen_share?: boolean | null;
}

export interface AgentActionLog {
  id: string;
  timestamp: number;
  action_type:
    | 'task'
    | 'thought'
    | 'reasoning'
    | 'tool_call'
    | 'tool_result'
    | 'permission_request'
    | 'finished'
    | 'error'
    | 'stopped'
    | 'compacted';
  description: string;
  details?: Record<string, any>;
}

export interface PermissionRequest {
  id: string;
  category: 'photos' | 'files' | 'messages' | 'mails' | 'destructive' | 'financial';
  title: string;
  reason: string;
  target_detail?: string;
}

export interface SettingsView {
  provider: string;
  aicredits_key: string;
  openai_key: string;
  anthropic_key: string;
  google_key: string;
  custom_key: string;
  tavily_key: string;
  model_name: string;
  base_url: string;
  vision_model: string;
  /** "low" | "medium" | "high", or "" for the model's default. */
  reasoning_effort: string;
  /** A key is saved for the provider in use. */
  has_key: boolean;
  has_tavily_key: boolean;
  ws_port: number;
  local_ip: string;
}

/** The phone this PC is paired with, and the pairing code while one is open. */
export interface PairingView {
  paired: { device_name: string; paired_at: number } | null;
  /** Like "7KQ4-M2XD-9HPA". */
  code: string | null;
  code_expires_at: number | null;
}

export interface ConnectionInfo {
  ip: string;
  port: number;
  ws_url: string;
}

export interface AgentStatus {
  enabled: boolean;
  running: boolean;
  paused: boolean;
}

export interface StreamEvent {
  step: number;
  kind: 'start' | 'content' | 'reasoning' | 'end';
  text: string;
}

export interface SessionMeta {
  id: string;
  title: string;
  created_at: number;
  updated_at: number;
}

export interface Session extends SessionMeta {
  summary?: string | null;
  logs: AgentActionLog[];
}

/** Fields left out keep their saved values. */
export type SaveSettings = Partial<
  Pick<
    SettingsView,
    | 'provider'
    | 'aicredits_key'
    | 'openai_key'
    | 'anthropic_key'
    | 'google_key'
    | 'custom_key'
    | 'tavily_key'
    | 'model_name'
    | 'base_url'
    | 'vision_model'
    | 'reasoning_effort'
  >
>;
