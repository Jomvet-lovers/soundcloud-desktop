export function isMac(): boolean {
  return !!(navigator.platform?.startsWith('Mac') || navigator.userAgent.includes('Mac'));
}
