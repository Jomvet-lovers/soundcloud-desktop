const WHITELIST = ['localhost', '127.0.0.1', 'tauri.localhost', 'scproxy.localhost', 'proxy.soundcloud.su', 'api.soundcloud.su', 'unpkg.com'];
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

// Hook <img>.src
const imgSrcDesc = Object.getOwnPropertyDescriptor(HTMLImageElement.prototype, 'src')!;
Object.defineProperty(HTMLImageElement.prototype, 'src', {
  set(url: string) {
    if (url?.startsWith('http') && !isWhitelisted(url)) url = scproxyUrl(url);
    imgSrcDesc.set!.call(this, url);
  },
  get() {
    return imgSrcDesc.get!.call(this);
  },
});

// Hook fetch()
const origFetch = window.fetch.bind(window);
window.fetch = ((input: RequestInfo | URL, init?: RequestInit) => {
  if (typeof input === 'string' && input.startsWith('http') && !isWhitelisted(input)) {
    input = scproxyUrl(input);
  } else if (input instanceof Request && input.url.startsWith('http') && !isWhitelisted(input.url)) {
    input = new Request(scproxyUrl(input.url), input);
  }
  return origFetch(input, init);
}) as typeof fetch;
