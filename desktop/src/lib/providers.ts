/**
 * Model providers Vivy can use. All of them speak the OpenAI chat-completions
 * API, so only the endpoint, key and model names differ. The endpoints live in
 * the Rust side (security/mod.rs); this list drives the UI.
 *
 * Mostly mid-tier models (capable enough for multi-step phone tasks, without
 * flagship prices) plus each provider's top model for complex tasks.
 * Every model listed accepts images and supports tool calls.
 */

export type ProviderId = 'aicredits' | 'openai' | 'anthropic' | 'google' | 'custom';

export type KeyField = 'aicredits_key' | 'openai_key' | 'anthropic_key' | 'google_key' | 'custom_key';

export interface ModelOption {
  id: string;
  name: string;
  /** What it's good for, shown as a tag. */
  note: string;
  /** Highlighted tag: the provider's everyday pick, or its top model. */
  tier?: 'everyday' | 'complex';
}

export interface Provider {
  id: ProviderId;
  name: string;
  keyField: KeyField;
  keyPlaceholder: string;
  /** Where to create a key. */
  keyUrl?: string;
  /** Thinking-effort levels the provider accepts, fastest first. Empty = no setting. */
  efforts: string[];
  /** Shown instead of the effort setting when there is none. */
  effortNote?: string;
  models: ModelOption[];
}

export const PROVIDERS: Provider[] = [
  {
    id: 'aicredits',
    name: 'AICredits',
    keyField: 'aicredits_key',
    keyPlaceholder: 'sk-...',
    keyUrl: 'https://aicredits.in',
    efforts: ['low', 'medium', 'high'],
    models: [
      { id: 'z-ai/glm-5.3-flash', name: 'GLM 5.3 Flash', note: 'Everyday', tier: 'everyday' },
      { id: 'anthropic/claude-haiku-5.5', name: 'Claude Haiku 5.5', note: 'Quick tasks' },
      { id: 'openai/gpt-6-luna', name: 'GPT-6 Luna', note: 'Quick tasks' },
      { id: 'deepseek/deepseek-v4.1-flash', name: 'DeepSeek V4.1 Flash', note: 'Everyday' },
      { id: 'moonshotai/kimi-k2.6', name: 'Kimi K2.6', note: 'Everyday' },
      { id: 'anthropic/claude-opus-5.5', name: 'Claude Opus 5.5', note: 'Complex tasks', tier: 'complex' },
    ],
  },
  {
    id: 'openai',
    name: 'OpenAI',
    keyField: 'openai_key',
    keyPlaceholder: 'sk-...',
    keyUrl: 'https://platform.openai.com/api-keys',
    efforts: ['none', 'minimal', 'low', 'medium', 'high', 'xhigh', 'max'],
    models: [
      { id: 'gpt-6.1-sol', name: 'GPT-6.1 Sol', note: 'Everyday', tier: 'everyday' },
      { id: 'gpt-6-luna', name: 'GPT-6 Luna', note: 'Quick tasks' },
      { id: 'gpt-5-mini', name: 'GPT-5 Mini', note: 'Simple tasks' },
      { id: 'gpt-6-astra', name: 'GPT-6 Astra', note: 'Complex tasks', tier: 'complex' },
    ],
  },
  {
    id: 'anthropic',
    name: 'Anthropic',
    keyField: 'anthropic_key',
    keyPlaceholder: 'sk-ant-...',
    keyUrl: 'https://console.anthropic.com/settings/keys',
    efforts: [],
    effortNote: 'Claude decides how much to think for each step.',
    models: [
      { id: 'claude-sonnet-5-5', name: 'Claude Sonnet 5.5', note: 'Everyday', tier: 'everyday' },
      { id: 'claude-sonnet-5', name: 'Claude Sonnet 5', note: 'Everyday' },
      { id: 'claude-haiku-5-5', name: 'Claude Haiku 5.5', note: 'Quick tasks' },
      { id: 'claude-opus-5-5', name: 'Claude Opus 5.5', note: 'Complex tasks', tier: 'complex' },
    ],
  },
  {
    id: 'google',
    name: 'Google Gemini',
    keyField: 'google_key',
    keyPlaceholder: 'AIza...',
    keyUrl: 'https://aistudio.google.com/apikey',
    efforts: ['minimal', 'low', 'medium', 'high'],
    models: [
      { id: 'gemini-3.8-flash', name: 'Gemini 3.8 Flash', note: 'Everyday', tier: 'everyday' },
      { id: 'gemini-3.6-flash', name: 'Gemini 3.6 Flash', note: 'Everyday' },
      { id: 'gemini-3.5-flash-lite', name: 'Gemini 3.5 Flash-Lite', note: 'Simple tasks' },
      { id: 'gemini-4-argon', name: 'Gemini 4 Argon', note: 'Complex tasks', tier: 'complex' },
    ],
  },
  {
    id: 'custom',
    name: 'Custom',
    keyField: 'custom_key',
    keyPlaceholder: 'API key',
    efforts: [],
    models: [],
  },
];

export function providerOf(id: string | undefined): Provider {
  return PROVIDERS.find((p) => p.id === id) ?? PROVIDERS[0];
}
