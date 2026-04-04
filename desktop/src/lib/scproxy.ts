import { getProxyPort } from './constants';

const WHITELIST = [
  'localhost',
  '127.0.0.1',
  'tauri.localhost',
  'scproxy.localhost',
  'proxy.soundcloud.su',
  'api.soundcloud.su',
  'unpkg.com',
];
const RETRY_BYPASS_CACHE_PARAM = '__scproxy_bust';
const LOCAL_PROXY_SHARDS = 20;

type PatchedImage = HTMLImageElement & {
  __origSrc?: string;
  __proxyRetryStage?: number;
  __skipProxyOnce?: boolean;
};

function isWhitelisted(url: string): boolean {
  try {
    const h = new URL(url).hostname;
    return WHITELIST.some((w) => h === w || h.endsWith(`.${w}`));
  } catch {
    return true;
  }
}

function withCacheBust(url: string): string {
  try {
    const next = new URL(url);
    next.searchParams.set(RETRY_BYPASS_CACHE_PARAM, `${Date.now()}`);
    return next.toString();
  } catch {
    const sep = url.includes('?') ? '&' : '?';
    return `${url}${sep}${RETRY_BYPASS_CACHE_PARAM}=${Date.now()}`;
  }
}

function hashShard(value: string): number {
  let hash = 0;
  for (let i = 0; i < value.length; i++) {
    hash = (hash * 33 + value.charCodeAt(i)) >>> 0;
  }
  return hash % LOCAL_PROXY_SHARDS;
}

function scproxyUrl(url: string, { bypassCache = false } = {}): string {
  const target = bypassCache ? withCacheBust(url) : url;
  const encoded = btoa(target);
  const proxyPort = getProxyPort();
  const encodedPath = encodeURIComponent(encoded);

  if (proxyPort) {
    const shard = hashShard(target);
    return `http://scproxy-${shard}.localhost:${proxyPort}/p/${encodedPath}`;
  }

  return `scproxy://localhost/${encodedPath}`;
}

// Hook <img>.src — store original URL to enable retry on error
const imgSrcDesc = Object.getOwnPropertyDescriptor(HTMLImageElement.prototype, 'src')!;
Object.defineProperty(HTMLImageElement.prototype, 'src', {
  set(url: string) {
    const img = this as PatchedImage;
    if (img.__skipProxyOnce) {
      img.__skipProxyOnce = false;
      imgSrcDesc.set!.call(this, url);
      return;
    }

    if (url?.startsWith('http') && !isWhitelisted(url)) {
      img.__origSrc = url;
      img.__proxyRetryStage = 0;
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
      const img = e.target as PatchedImage;
      const originalUrl = img.__origSrc;
      const retryStage = img.__proxyRetryStage ?? 0;

      if (originalUrl && retryStage === 0) {
        img.__proxyRetryStage = 1;
        img.style.removeProperty('display');
        imgSrcDesc.set!.call(img, scproxyUrl(originalUrl, { bypassCache: true }));
        return;
      }

      if (originalUrl && retryStage === 1) {
        img.__proxyRetryStage = 2;
        img.__skipProxyOnce = true;
        img.style.removeProperty('display');
        imgSrcDesc.set!.call(img, originalUrl);
        return;
      }

      img.style.display = 'none';
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
