// Isolated module to avoid circular deps: api-client ↔ subscription
let cachedPremium = false;

export function getIsPremium(): boolean {
  return cachedPremium;
}

export function setIsPremium(value: boolean): void {
  cachedPremium = value;
}
