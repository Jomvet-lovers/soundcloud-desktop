interface CallbackPageParams {
  success: boolean;
  sessionId?: string;
  username?: string | null;
  error?: string;
}

export function renderCallbackPage(params: CallbackPageParams): string {
  const { success, sessionId, username, error } = params;

  return `<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>SoundCloud Desktop — ${success ? 'Connected' : 'Error'}</title>
  <style>
    * { margin: 0; padding: 0; box-sizing: border-box; }

    body {
      min-height: 100vh;
      display: flex;
      align-items: center;
      justify-content: center;
      font-family: -apple-system, BlinkMacSystemFont, 'SF Pro Display', 'Segoe UI', Roboto, sans-serif;
      background: linear-gradient(135deg, #0a0a0a 0%, #1a1a2e 50%, #16213e 100%);
      color: #fff;
      overflow: hidden;
    }

    body::before {
      content: '';
      position: fixed;
      top: -50%;
      left: -50%;
      width: 200%;
      height: 200%;
      background: radial-gradient(circle at 30% 40%, rgba(255, 85, 0, 0.08) 0%, transparent 50%),
                  radial-gradient(circle at 70% 60%, rgba(138, 43, 226, 0.06) 0%, transparent 50%),
                  radial-gradient(circle at 50% 50%, rgba(0, 150, 255, 0.04) 0%, transparent 50%);
      animation: aurora 15s ease-in-out infinite alternate;
      z-index: 0;
    }

    @keyframes aurora {
      0% { transform: translate(0, 0) rotate(0deg); }
      100% { transform: translate(-5%, -5%) rotate(3deg); }
    }

    .card {
      position: relative;
      z-index: 1;
      width: 420px;
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
      animation: slideUp 0.6s cubic-bezier(0.16, 1, 0.3, 1) forwards;
      opacity: 0;
      transform: translateY(20px);
    }

    @keyframes slideUp {
      to { opacity: 1; transform: translateY(0); }
    }

    .icon {
      width: 72px;
      height: 72px;
      margin: 0 auto 24px;
      border-radius: 50%;
      display: flex;
      align-items: center;
      justify-content: center;
      font-size: 36px;
      animation: popIn 0.5s cubic-bezier(0.16, 1, 0.3, 1) 0.3s forwards;
      opacity: 0;
      transform: scale(0.5);
    }

    .icon.success {
      background: linear-gradient(135deg, rgba(52, 199, 89, 0.2), rgba(52, 199, 89, 0.05));
      border: 1px solid rgba(52, 199, 89, 0.3);
    }

    .icon.error {
      background: linear-gradient(135deg, rgba(255, 69, 58, 0.2), rgba(255, 69, 58, 0.05));
      border: 1px solid rgba(255, 69, 58, 0.3);
    }

    @keyframes popIn {
      to { opacity: 1; transform: scale(1); }
    }

    h1 {
      font-size: 24px;
      font-weight: 600;
      letter-spacing: -0.02em;
      margin-bottom: 8px;
    }

    .subtitle {
      font-size: 15px;
      color: rgba(255, 255, 255, 0.55);
      line-height: 1.5;
      margin-bottom: 32px;
    }

    .username {
      color: rgba(255, 255, 255, 0.9);
      font-weight: 500;
    }

    .error-msg {
      font-size: 13px;
      color: rgba(255, 69, 58, 0.9);
      background: rgba(255, 69, 58, 0.08);
      border: 1px solid rgba(255, 69, 58, 0.15);
      border-radius: 12px;
      padding: 12px 16px;
      margin-bottom: 24px;
      word-break: break-word;
    }

    .hint {
      font-size: 13px;
      color: rgba(255, 255, 255, 0.35);
      margin-top: 24px;
    }

    .session-id {
      display: none;
    }
  </style>
</head>
<body>
  <div class="card">
    ${
      success
        ? `
      <div class="icon success">&#10003;</div>
      <h1>Connected</h1>
      <p class="subtitle">
        ${username ? `Signed in as <span class="username">${escapeHtml(username)}</span>` : 'Successfully authenticated with SoundCloud'}
      </p>
      <p class="hint">You can return to the app now</p>
      <span class="session-id" data-session-id="${sessionId || ''}"></span>
    `
        : `
      <div class="icon error">&#10007;</div>
      <h1>Connection Failed</h1>
      <p class="subtitle">Could not authenticate with SoundCloud</p>
      ${error ? `<div class="error-msg">${escapeHtml(error)}</div>` : ''}
      <p class="hint">Please close this window and try again</p>
    `
    }
  </div>
</body>
</html>`;
}

function escapeHtml(str: string): string {
  return str
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#039;');
}
