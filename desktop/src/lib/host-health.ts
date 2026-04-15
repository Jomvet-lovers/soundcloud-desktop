const UNHEALTHY_DURATION_MS = 30_000;
const unhealthyUntil = new Map<string, number>();

export function markUnhealthy(host: string): void {
  unhealthyUntil.set(host, Date.now() + UNHEALTHY_DURATION_MS);
}

export function markHealthy(host: string): void {
  unhealthyUntil.delete(host);
}

export function isHealthy(host: string): boolean {
  const until = unhealthyUntil.get(host);
  if (until === undefined) return true;
  if (Date.now() > until) {
    unhealthyUntil.delete(host);
    return true;
  }
  return false;
}
