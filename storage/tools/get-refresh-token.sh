#!/usr/bin/env bash
set -euo pipefail

# Однократный CLI-flow OAuth для получения refresh_token Google Drive под Desktop-app.
#
# Usage:
#   ./get-refresh-token.sh <CLIENT_ID> <CLIENT_SECRET>
#
# Где взять CLIENT_ID и CLIENT_SECRET (один раз на проект GCP):
#
#   1. https://console.cloud.google.com/projectcreate
#      → создать проект (название любое).
#
#   2. APIs & Services → Library → "Google Drive API" → Enable.
#
#   3. APIs & Services → OAuth consent screen
#      → User Type: External → Create.
#      → App name: storage-gdrive, support email: твой.
#      → Scopes: пропустить (запросим drive scope в URL ниже).
#      → Test users: добавить email того аккаунта, под которым будем
#        логиниться в п.6 (без этого refresh_token истечёт через 7 дней).
#      → (опционально) Publish app → переводит в Production: токен не истекает.
#
#   4. APIs & Services → Credentials → Create Credentials → OAuth client ID
#      → Application type: Desktop app
#      → Name: storage-gdrive-cli → Create.
#      → В появившемся диалоге будут CLIENT_ID и CLIENT_SECRET — это и есть
#        аргументы для этого скрипта.
#
#   5. Запустить:
#        ./get-refresh-token.sh <CLIENT_ID> <CLIENT_SECRET>
#
#   6. Скрипт распечатает URL и откроет его в браузере. Залогиниться под
#      "техническим" аккаунтом (тем, чьи refresh_token нужны), нажать Continue
#      на warning-экране ("This app isn't verified" → Advanced → Go to ...),
#      разрешить доступ к Drive. Браузер вернётся на http://127.0.0.1:<port>/
#      — скрипт поймает code и обменяет на refresh_token.
#
#   7. На выходе три строки env: GDRIVE_OAUTH_{CLIENT_ID,CLIENT_SECRET,REFRESH_TOKEN}
#      — копи-пастишь в docker-compose.yml на хосте.
#
# Требует: curl, jq, python3 (для подъёма локального loopback), xdg-open или ручное
# открытие URL в браузере.

if [[ $# -lt 2 ]]; then
    sed -n '/^# Usage:/,/^# Требует:/p' "$0" | sed 's/^# \?//' >&2
    echo "usage: $0 <CLIENT_ID> <CLIENT_SECRET>" >&2
    exit 1
fi

CLIENT_ID="$1"
CLIENT_SECRET="$2"
SCOPE="https://www.googleapis.com/auth/drive"
REDIRECT="urn:ietf:wg:oauth:2.0:oob"

# Google задеприкейтил OOB в 2022, но Desktop-app по-прежнему может использовать
# loopback-IP redirect: http://127.0.0.1:<PORT>/. Поднимаем одноразовый python-сервер.

PORT=$(python3 -c "import socket; s=socket.socket(); s.bind(('127.0.0.1', 0)); print(s.getsockname()[1]); s.close()")
REDIRECT="http://127.0.0.1:${PORT}/"

AUTH_URL="https://accounts.google.com/o/oauth2/v2/auth?\
client_id=${CLIENT_ID}\
&redirect_uri=${REDIRECT}\
&response_type=code\
&scope=${SCOPE}\
&access_type=offline\
&prompt=consent"

cat <<EOF
=========================================================================
Открой URL в браузере (если не открылся автоматически), залогинься под
тем Google-аккаунтом, чьи refresh_token хочешь получить, разреши доступ.
Браузер уйдёт на http://127.0.0.1:${PORT}/?code=... — этот скрипт его поймает.
=========================================================================

${AUTH_URL}

EOF

if command -v xdg-open >/dev/null 2>&1; then xdg-open "$AUTH_URL" >/dev/null 2>&1 || true; fi

CODE=$(python3 - <<PY
import http.server, urllib.parse, sys
code = {"v": None}
class H(http.server.BaseHTTPRequestHandler):
    def log_message(self, *a, **kw): pass
    def do_GET(self):
        q = urllib.parse.urlparse(self.path).query
        code["v"] = urllib.parse.parse_qs(q).get("code", [None])[0]
        self.send_response(200)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.end_headers()
        self.wfile.write(b"<html><body><h2>OK, code received. You can close this tab.</h2></body></html>")
srv = http.server.HTTPServer(("127.0.0.1", $PORT), H)
srv.handle_request()
print(code["v"] or "", end="")
PY
)

if [[ -z "$CODE" ]]; then
    echo "no code received" >&2
    exit 2
fi

echo "exchanging code for tokens..."
RESP=$(curl -fsSL -X POST https://oauth2.googleapis.com/token \
    -d "code=${CODE}" \
    -d "client_id=${CLIENT_ID}" \
    -d "client_secret=${CLIENT_SECRET}" \
    -d "redirect_uri=${REDIRECT}" \
    -d "grant_type=authorization_code")

REFRESH=$(echo "$RESP" | jq -r '.refresh_token // empty')
ACCESS=$(echo "$RESP" | jq -r '.access_token // empty')

if [[ -z "$REFRESH" ]]; then
    echo "ERROR: no refresh_token in response. Full body:" >&2
    echo "$RESP" >&2
    exit 3
fi

cat <<EOF

=========================================================================
SUCCESS. Save these into your storage env:

GDRIVE_OAUTH_CLIENT_ID=${CLIENT_ID}
GDRIVE_OAUTH_CLIENT_SECRET=${CLIENT_SECRET}
GDRIVE_OAUTH_REFRESH_TOKEN=${REFRESH}
=========================================================================
EOF
