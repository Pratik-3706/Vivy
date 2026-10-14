const KEY = 'vivy.history';
const MAX = 30;

/** Recent prompts, newest first. Per-machine convenience only. */
export function loadHistory(): string[] {
  try {
    const raw = localStorage.getItem(KEY);
    const parsed = raw ? JSON.parse(raw) : [];
    return Array.isArray(parsed) ? parsed.filter((p) => typeof p === 'string') : [];
  } catch {
    return [];
  }
}

export function pushHistory(prompt: string): string[] {
  const next = [prompt, ...loadHistory().filter((p) => p !== prompt)].slice(0, MAX);
  try {
    localStorage.setItem(KEY, JSON.stringify(next));
  } catch {
    // storage unavailable; history just won't persist
  }
  return next;
}
