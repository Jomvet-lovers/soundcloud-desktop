const WHITELIST = [
  'localhost',
  '127.0.0.1',
  'tauri.localhost',
  'scproxy.localhost',
  'proxy.soundcloud.su',
  'api.soundcloud.su',
  'unpkg.com',
];
const IS_WINDOWS = navigator.userAgent.includes('Windows');

function isWhitelisted(url: string): boolean {
  try {
    const h = new URL(url).hostname;
    return WHITELIST.some((w) => h === w || h.endsWith(`.${w}`));
  } catch {
    return true;
  }
}

function scproxyUrl(url: string): string {
  const encoded = btoa(url);
  return IS_WINDOWS ? `http://scproxy.localhost/${encoded}` : `scproxy://localhost/${encoded}`;
}

// Hook <img>.src — store original URL to enable retry on error
const imgSrcDesc = Object.getOwnPropertyDescriptor(HTMLImageElement.prototype, 'src')!;
Object.defineProperty(HTMLImageElement.prototype, 'src', {
  set(url: string) {
    if (url?.startsWith('http') && !isWhitelisted(url)) {
      (this as HTMLImageElement & { __origSrc: string }).__origSrc = url;
      url = scproxyUrl(url);
    }
    imgSrcDesc.set!.call(this, url);
  },
  get() {
    return imgSrcDesc.get!.call(this);
  },
});

// Global: hide broken images (proxy error, CDN blocked, etc.)
document.addEventListener(
  'error',
  (e) => {
    if (e.target instanceof HTMLImageElement) {
      e.target.style.display = 'none';
    }
  },
  true,
);

// Hook fetch()
const origFetch = window.fetch.bind(window);
window.fetch = ((input: RequestInfo | URL, init?: RequestInit) => {
  if (typeof input === 'string' && input.startsWith('http') && !isWhitelisted(input)) {
    input = scproxyUrl(input);
  } else if (
    input instanceof Request &&
    input.url.startsWith('http') &&
    !isWhitelisted(input.url)
  ) {
    input = new Request(scproxyUrl(input.url), input);
  }
  return origFetch(input, init);
}) as typeof fetch;

export {};
