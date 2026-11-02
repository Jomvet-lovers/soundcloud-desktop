import { invoke as coreInvoke } from '@tauri-apps/api/core';

const EVENT_LOOP_TICK_MS = 1000;
const EVENT_LOOP_WARN_MS = 500;
const INVOKE_WARN_MS = 1500;
const ASYNC_WARN_MS = 2500;

let watchdogStarted = false;

function roundMs(value: number) {
  return Math.round(value);
}

function writeLog(level: 'INFO' | 'WARN' | 'ERROR', message: string) {
  void coreInvoke('diagnostics_log', { level, message }).catch(() => undefined);
}

export function logInfo(message: string) {
  console.info(message);
  writeLog('INFO', message);
}

function logWarn(message: string) {
  console.warn(message);
  writeLog('WARN', message);
}

function logError(message: string) {
  console.error(message);
  writeLog('ERROR', message);
}

export function setupUiWatchdog() {
  if (watchdogStarted) return;
  watchdogStarted = true;

  logInfo('[Perf] UI watchdog started');

  let expectedAt = performance.now() + EVENT_LOOP_TICK_MS;
  window.setInterval(() => {
    const now = performance.now();
    const lag = now - expectedAt;
    expectedAt = now + EVENT_LOOP_TICK_MS;

    if (document.visibilityState === 'visible' && lag > EVENT_LOOP_WARN_MS) {
      logWarn(`[Perf] UI event loop lag detected: ${roundMs(lag)}ms`);
    }
  }, EVENT_LOOP_TICK_MS);

  window.addEventListener('error', (event) => {
    logError(`[UI] Unhandled error: ${event.message}`);
  });

  window.addEventListener('unhandledrejection', (event) => {
    logError(`[UI] Unhandled rejection: ${String(event.reason)}`);
  });
}

function truncate(value: string, max = 500) {
  return value.length > max ? `${value.slice(0, max)}...[+${value.length - max}]` : value;
}

// Transport errors (@tauri-apps/plugin-http → reqwest) arrive as one
// flat Error: the top-level Display reads "error sending request for url (...)",
// while the real cause (timeout / dns / connection refused / tcp connect)
// hides in the source() chain, which the plugin partially forwards into .cause.
// String(error) loses it — so we unwrap the name + the whole cause chain.
export function describeError(error: unknown): string {
  if (!(error instanceof Error)) return truncate(String(error));

  const parts: string[] = [];
  let cur: unknown = error;
  const seen = new Set<unknown>();
  while (cur instanceof Error && !seen.has(cur)) {
    seen.add(cur);
    const name = cur.name && cur.name !== 'Error' ? `${cur.name}: ` : '';
    parts.push(`${name}${cur.message || '(no message)'}`);
    cur = (cur as { cause?: unknown }).cause;
  }
  if (cur != null && !(cur instanceof Error)) parts.push(String(cur));

  // AbortError = our client-side timeout fired (see fetchWithTimeout),
  // not a network failure. That changes the diagnosis, so mark it explicitly.
  if (error.name === 'AbortError') parts.push('(client-timeout: our AbortController fired)');

  return truncate(parts.join(' -> '));
}

export function logHttpError(
  label: string,
  status: number,
  url: string,
  body?: string,
  error?: unknown,
) {
  const parts = [`[Perf] HTTP ${status} ${label} ${url}`];
  if (body) parts.push(`body=${truncate(body)}`);
  if (error !== undefined) parts.push(`error=${describeError(error)}`);
  logError(parts.join(' | '));
}

export function logHttpFailure(label: string, url: string, error: unknown, elapsedMs?: number) {
  const parts = [`[Perf] HTTP FAIL ${label} ${url}`];
  if (elapsedMs !== undefined) parts.push(`after=${roundMs(elapsedMs)}ms`);
  parts.push(`error=${describeError(error)}`);
  logError(parts.join(' | '));
}

export async function trackAsync<T>(
  label: string,
  promise: Promise<T>,
  warnMs = ASYNC_WARN_MS,
): Promise<T> {
  const startedAt = performance.now();
  const slowTimer = window.setTimeout(() => {
    logWarn(`[Perf] Slow task still running: ${label} (${warnMs}ms+)`);
  }, warnMs);

  try {
    return await promise;
  } catch (error) {
    const elapsed = performance.now() - startedAt;
    logError(`[Perf] Task failed: ${label} (after ${roundMs(elapsed)}ms): ${describeError(error)}`);
    throw error;
  } finally {
    window.clearTimeout(slowTimer);
    const elapsed = performance.now() - startedAt;
    if (elapsed > warnMs) {
      logWarn(`[Perf] Slow task finished: ${label} (${roundMs(elapsed)}ms)`);
    }
  }
}

export function trackedInvoke<T>(
  command: string,
  args?: Record<string, unknown>,
  warnMs = INVOKE_WARN_MS,
): Promise<T> {
  return trackAsync(`invoke:${command}`, coreInvoke<T>(command, args), warnMs);
}
