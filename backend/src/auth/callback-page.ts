interface CallbackPageParams {
  /** id для polling /auth/login/status. null = state не найден, рендерим только error. */
  loginRequestId: string | null;
  initialStatus: 'pending' | 'completed' | 'failed';
  username?: string | null;
  error?: string;
}

export function renderCallbackPage(params: CallbackPageParams): string {
  const { loginRequestId, initialStatus, username, error } = params;

  // Сериализуем initial state как JSON: безопасно, JSON.stringify экранирует все спецсимволы.
  // На клиенте читаем только через textContent / element creation, без innerHTML.
  const initialPayload = JSON.stringify({
    loginRequestId,
    initialStatus,
    username: username ?? null,
    error: error ?? null,
  }).replace(/</g, '\\u003c');

  return `<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>SoundCloud Desktop</title>
  <style>
    * { margin: 0; padding: 0; box-sizing: border-box; }

    html, body {
      min-height: 100vh;
      font-family: -apple-system, BlinkMacSystemFont, 'SF Pro Display', 'Segoe UI', Roboto, sans-serif;
      background: linear-gradient(135deg, #0a0a0a 0%, #1a1a2e 50%, #16213e 100%);
      color: #fff;
      overflow: hidden;
    }

    body {
      display: flex;
      align-items: center;
      justify-content: center;
      position: relative;
    }

    body::before {
      content: '';
      position: fixed;
      inset: -50%;
      width: 200%;
      height: 200%;
      background:
        radial-gradient(circle at 30% 40%, rgba(255, 85, 0, 0.10) 0%, transparent 50%),
        radial-gradient(circle at 70% 60%, rgba(138, 43, 226, 0.07) 0%, transparent 50%),
        radial-gradient(circle at 50% 50%, rgba(0, 150, 255, 0.05) 0%, transparent 50%);
      animation: aurora 18s ease-in-out infinite alternate;
      z-index: 0;
      will-change: transform;
    }

    @keyframes aurora {
      0%   { transform: translate(0, 0) rotate(0deg); }
      100% { transform: translate(-5%, -5%) rotate(3deg); }
    }

    .card {
      position: relative;
      z-index: 1;
      width: 420px;
      max-width: calc(100vw - 32px);
      padding: 48px 40px;
      border-radius: 24px;
      background: rgba(255, 255, 255, 0.06);
      backdrop-filter: blur(40px) saturate(1.8);
      -webkit-backdrop-filter: blur(40px) saturate(1.8);
      border: 1px solid rgba(255, 255, 255, 0.12);
      box-shadow:
        0 8px 32px rgba(0, 0, 0, 0.4),
        inset 0 1px 0 rgba(255, 255, 255, 0.1);
      text-align: center;
      animation: slideUp 0.6s cubic-bezier(0.16, 1, 0.3, 1) both;
    }

    @keyframes slideUp {
      from { opacity: 0; transform: translateY(20px); }
      to   { opacity: 1; transform: translateY(0); }
    }

    .icon {
      width: 72px;
      height: 72px;
      margin: 0 auto 24px;
      border-radius: 50%;
      display: flex;
      align-items: center;
      justify-content: center;
    }

    .icon.loading {
      background: linear-gradient(135deg, rgba(255, 255, 255, 0.08), rgba(255, 255, 255, 0.02));
      border: 1px solid rgba(255, 255, 255, 0.12);
      position: relative;
    }
    .icon.loading::after {
      content: '';
      position: absolute;
      inset: -2px;
      border-radius: 50%;
      border: 2px solid transparent;
      border-top-color: #ff7b3a;
      border-right-color: rgba(255, 123, 58, 0.4);
      animation: spin 1s linear infinite;
    }

    .icon.success {
      background: linear-gradient(135deg, rgba(52, 199, 89, 0.25), rgba(52, 199, 89, 0.05));
      border: 1px solid rgba(52, 199, 89, 0.35);
      animation: popIn 0.5s cubic-bezier(0.16, 1, 0.3, 1);
    }

    .icon.error {
      background: linear-gradient(135deg, rgba(255, 69, 58, 0.22), rgba(255, 69, 58, 0.05));
      border: 1px solid rgba(255, 69, 58, 0.32);
      animation: popIn 0.5s cubic-bezier(0.16, 1, 0.3, 1);
    }

    @keyframes spin { to { transform: rotate(360deg); } }
    @keyframes popIn {
      0%   { transform: scale(0.5); opacity: 0; }
      60%  { transform: scale(1.08); opacity: 1; }
      100% { transform: scale(1); opacity: 1; }
    }

    .icon svg { width: 36px; height: 36px; fill: none; stroke-width: 2.5; stroke-linecap: round; stroke-linejoin: round; }
    .icon.success svg { stroke: rgb(52, 199, 89); }
    .icon.error   svg { stroke: rgb(255, 99, 88); }

    h1 {
      font-size: 22px;
      font-weight: 600;
      letter-spacing: -0.02em;
      margin-bottom: 10px;
    }

    .subtitle {
      font-size: 14px;
      color: rgba(255, 255, 255, 0.55);
      line-height: 1.5;
      margin-bottom: 24px;
      min-height: 21px;
    }

    .username {
      color: rgba(255, 255, 255, 0.92);
      font-weight: 500;
    }

    .steps {
      display: flex;
      flex-direction: column;
      gap: 8px;
      margin-bottom: 24px;
      text-align: left;
    }

    .step {
      display: flex;
      align-items: center;
      gap: 10px;
      padding: 8px 12px;
      border-radius: 10px;
      background: rgba(255, 255, 255, 0.03);
      border: 1px solid rgba(255, 255, 255, 0.05);
      font-size: 12.5px;
      color: rgba(255, 255, 255, 0.4);
      transition: all 0.4s cubic-bezier(0.16, 1, 0.3, 1);
    }

    .step .dot {
      width: 7px;
      height: 7px;
      border-radius: 50%;
      background: rgba(255, 255, 255, 0.15);
      flex-shrink: 0;
      transition: all 0.4s ease;
    }

    .step.active {
      background: rgba(255, 123, 58, 0.07);
      border-color: rgba(255, 123, 58, 0.18);
      color: rgba(255, 255, 255, 0.85);
    }
    .step.active .dot {
      background: #ff7b3a;
      box-shadow: 0 0 12px rgba(255, 123, 58, 0.65);
      animation: pulse 1.4s ease-in-out infinite;
    }

    .step.done { color: rgba(255, 255, 255, 0.5); }
    .step.done .dot {
      background: rgb(52, 199, 89);
      box-shadow: 0 0 8px rgba(52, 199, 89, 0.4);
    }

    @keyframes pulse {
      0%, 100% { transform: scale(1); opacity: 1; }
      50%      { transform: scale(1.4); opacity: 0.65; }
    }

    .error-msg {
      font-size: 12.5px;
      color: rgba(255, 99, 88, 0.95);
      background: rgba(255, 69, 58, 0.08);
      border: 1px solid rgba(255, 69, 58, 0.18);
      border-radius: 12px;
      padding: 12px 16px;
      margin-bottom: 20px;
      word-break: break-word;
      text-align: left;
      animation: slideUp 0.4s cubic-bezier(0.16, 1, 0.3, 1) both;
    }

    .hint {
      font-size: 12px;
      color: rgba(255, 255, 255, 0.32);
      margin-top: 16px;
    }
  </style>
</head>
<body>
  <div class="card">
    <div class="icon loading" id="icon"></div>
    <h1 id="title">Connecting…</h1>
    <p class="subtitle" id="subtitle">Hang tight, finishing up the handshake with SoundCloud.</p>

    <div class="steps" id="steps">
      <div class="step active" data-step="exchange"><span class="dot"></span><span>Exchanging authorization code</span></div>
      <div class="step" data-step="profile"><span class="dot"></span><span>Fetching your profile</span></div>
      <div class="step" data-step="finalize"><span class="dot"></span><span>Finalizing session</span></div>
    </div>

    <div id="errorBox"></div>
    <p class="hint" id="hint"></p>
  </div>

  <script id="payload" type="application/json">${initialPayload}</script>
  <script>
  (function () {
    var data = JSON.parse(document.getElementById('payload').textContent);

    var SVG_NS = 'http://www.w3.org/2000/svg';

    function makeSvg(paths) {
      var svg = document.createElementNS(SVG_NS, 'svg');
      svg.setAttribute('viewBox', '0 0 24 24');
      paths.forEach(function (p) {
        var el = document.createElementNS(SVG_NS, p.tag);
        Object.keys(p.attrs).forEach(function (k) { el.setAttribute(k, p.attrs[k]); });
        svg.appendChild(el);
      });
      return svg;
    }

    function checkIcon() {
      return makeSvg([{ tag: 'polyline', attrs: { points: '20 6 9 17 4 12' } }]);
    }
    function crossIcon() {
      return makeSvg([
        { tag: 'line', attrs: { x1: '18', y1: '6', x2: '6', y2: '18' } },
        { tag: 'line', attrs: { x1: '6', y1: '6', x2: '18', y2: '18' } },
      ]);
    }

    var icon = document.getElementById('icon');
    var title = document.getElementById('title');
    var subtitle = document.getElementById('subtitle');
    var hint = document.getElementById('hint');
    var stepsBox = document.getElementById('steps');
    var errorBox = document.getElementById('errorBox');

    function setStep(name) {
      var nodes = stepsBox.querySelectorAll('.step');
      var hit = false;
      nodes.forEach(function (n) {
        if (hit) {
          n.classList.remove('active', 'done');
        } else if (n.dataset.step === name) {
          n.classList.add('active');
          n.classList.remove('done');
          hit = true;
        } else {
          n.classList.remove('active');
          n.classList.add('done');
        }
      });
    }

    function showError(msg) {
      icon.classList.remove('loading', 'success');
      icon.classList.add('error');
      icon.replaceChildren(crossIcon());
      title.textContent = 'Connection Failed';
      subtitle.textContent = "Couldn't authenticate with SoundCloud.";
      stepsBox.style.display = 'none';
      errorBox.replaceChildren();
      if (msg) {
        var box = document.createElement('div');
        box.className = 'error-msg';
        box.textContent = msg;
        errorBox.appendChild(box);
      }
      hint.textContent = 'Please close this window and try again.';
    }

    function showSuccess(name) {
      icon.classList.remove('loading', 'error');
      icon.classList.add('success');
      icon.replaceChildren(checkIcon());
      title.textContent = 'Connected';
      subtitle.replaceChildren();
      if (name) {
        subtitle.appendChild(document.createTextNode('Signed in as '));
        var u = document.createElement('span');
        u.className = 'username';
        u.textContent = name;
        subtitle.appendChild(u);
      } else {
        subtitle.textContent = 'Successfully authenticated with SoundCloud.';
      }
      stepsBox.querySelectorAll('.step').forEach(function (n) {
        n.classList.remove('active');
        n.classList.add('done');
      });
      hint.textContent = 'You can return to the app now.';
    }

    if (!data.loginRequestId || data.initialStatus === 'failed') {
      showError(data.error || 'Authentication failed.');
      return;
    }
    if (data.initialStatus === 'completed') {
      showSuccess(data.username);
      return;
    }

    // Pending — анимируем шаги и polling'уем.
    var stepIdx = 0;
    var stepNames = ['exchange', 'profile', 'finalize'];
    var stepTimer = setInterval(function () {
      if (stepIdx < stepNames.length - 1) {
        stepIdx += 1;
        setStep(stepNames[stepIdx]);
      }
    }, 1200);

    var pollAttempts = 0;
    var maxAttempts = 90;

    function poll() {
      pollAttempts += 1;
      fetch('/auth/login/status?id=' + encodeURIComponent(data.loginRequestId), { cache: 'no-store' })
        .then(function (r) { return r.ok ? r.json() : null; })
        .then(function (s) {
          if (!s) { schedule(); return; }
          if (s.status === 'completed') {
            clearInterval(stepTimer);
            if (s.sessionId) {
              fetch('/auth/session', { headers: { 'x-session-id': s.sessionId }, cache: 'no-store' })
                .then(function (r) { return r.ok ? r.json() : null; })
                .then(function (sess) { showSuccess(sess && sess.username); })
                .catch(function () { showSuccess(null); });
            } else {
              showSuccess(null);
            }
            return;
          }
          if (s.status === 'failed' || s.status === 'expired') {
            clearInterval(stepTimer);
            showError(s.error || 'Authentication failed.');
            return;
          }
          schedule();
        })
        .catch(function () { schedule(); });
    }

    function schedule() {
      if (pollAttempts >= maxAttempts) {
        clearInterval(stepTimer);
        showError('Authentication is taking too long. Please try again.');
        return;
      }
      setTimeout(poll, 700);
    }

    setTimeout(poll, 400);
  })();
  </script>
</body>
</html>`;
}
