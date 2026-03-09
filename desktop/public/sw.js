/** @type {ServiceWorkerGlobalScope} */
const sw = /** @type {any} */ (self);

/** @type {string[]} */
const WHITELIST = ['localhost', '127.0.0.1', 'tauri.localhost', 'backend.soundcloud.work.gd', 'soundcloud.work.gd', 'unpkg.com'];
/** @type {string | null} */
const PORT = new URL(sw.location.href).searchParams.get('port');

sw.addEventListener('install', () => sw.skipWaiting());
sw.addEventListener('activate', (e) => e.waitUntil(sw.clients.claim()));

sw.addEventListener('fetch', (event) => {
    const url = new URL(event.request.url);

    if (url.protocol !== 'http:' && url.protocol !== 'https:') return;
    if (WHITELIST.some((w) => url.hostname === w)) return;
    if (!PORT) return;

    event.respondWith(proxyRequest(event.request));
});

/**
 * @param {Request} request
 * @returns {Promise<Response>}
 */
async function proxyRequest(request) {
    const proxyUrl = `http://127.0.0.1:${PORT}/p/${btoa(request.url)}`;

    /** @type {RequestInit} */
    const init = {
        method: request.method,
        headers: {},
    };

    // Forward relevant headers
    for (const [key, value] of request.headers) {
        const k = key.toLowerCase();
        if (['content-type', 'range', 'accept', 'accept-encoding', 'authorization'].includes(k)) {
            init.headers[k] = value;
        }
    }

    // Forward body for non-GET/HEAD
    if (request.method !== 'GET' && request.method !== 'HEAD') {
        init.body = await request.arrayBuffer();
    }

    try {
        const res = await fetch(proxyUrl, init);
        if (res.ok || res.status === 206) {
            return new Response(res.body, {
                status: res.status,
                statusText: res.statusText,
                headers: res.headers,
            });
        }
    } catch {
    }

    // Fallback: direct request
    return fetch(request);
}