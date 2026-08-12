# Desktop/App

Страницы-конструктор, общие для win/mac/linux. Web-бандл (vite) — рендер Linux-шелла
(Servo) и дев-превью в браузере.

## Дев-превью

```bash
# 1. rpc-фасад ядра (печатает {"port":…,"token":…})
cd ../bindings && cargo run -p sc-rpc -- \
  --data-dir ~/.local/share/soundcloud-dev/data \
  --cache-dir ~/.local/share/soundcloud-dev/cache --port 7799

# 2. превью
pnpm dev

# 3. открыть
http://localhost:5199/#p=<port>;t=<token>
```

Сессия ядра живёт в `--data-dir` — логин через превью (кнопка → OAuth в браузере)
переживает перезапуски.
