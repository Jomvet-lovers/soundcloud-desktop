import { isWhitelistedAssetUrl, toScproxyUrl } from './asset-url';

type PatchedImage = HTMLImageElement & {
  __origSrc?: string;
  __proxyRetryStage?: number;
  __skipProxyOnce?: boolean;
};

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

    if (url?.startsWith('http') && !isWhitelistedAssetUrl(url)) {
      img.__origSrc = url;
      img.__proxyRetryStage = 0;
      url = toScproxyUrl(url);
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
        imgSrcDesc.set!.call(img, toScproxyUrl(originalUrl, { bypassCache: true }));
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
  if (typeof input === 'string' && input.startsWith('http') && !isWhitelistedAssetUrl(input)) {
    input = toScproxyUrl(input);
  } else if (
    input instanceof Request &&
    input.url.startsWith('http') &&
    !isWhitelistedAssetUrl(input.url)
  ) {
    input = new Request(toScproxyUrl(input.url), input);
  }
  return origFetch(input, init);
}) as typeof fetch;
