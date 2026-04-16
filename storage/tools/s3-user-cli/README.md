# s3-user.sh — CLI для управления S3-бакетами пользователей

Нейтральная обёртка над `aws-cli` для любых S3-совместимых хранилищ (AWS S3, MinIO, Cloud.ru, Yandex Object Storage, DigitalOcean Spaces и т.д.).
Каждому юзеру — отдельный бакет с именем `<prefix><slug>` (префикс и endpoint настраиваются через env).

---

## Требования

- `aws-cli` v1 или v2 (`aws --version`)
- bash ≥ 4

Установка aws-cli:

```bash
# Arch Linux (рекомендую v2)
sudo pacman -S aws-cli-v2
# либо v1
sudo pacman -S aws-cli

# Debian/Ubuntu
sudo apt install awscli

# Fedora
sudo dnf install awscli2

# macOS
brew install awscli

# Standalone-бинарь (любой Linux x86_64)
curl "https://awscli.amazonaws.com/awscli-exe-linux-x86_64.zip" -o /tmp/awscli.zip
unzip /tmp/awscli.zip -d /tmp && sudo /tmp/aws/install
```

Проверка:
```bash
aws --version
```

Если `aws: command not found` после установки — перезапусти шелл или проверь `PATH`.

---

## Настройка

Скопируй шаблон и заполни:

```bash
cp storage/tools/s3-user-cli/.env.example storage/tools/s3-user-cli/.env
# заполнить AWS_ACCESS_KEY_ID / AWS_SECRET_ACCESS_KEY (+ endpoint, если не AWS)
set -a; source storage/tools/s3-user-cli/.env; set +a
```

Либо напрямую экспортировать:

```bash
export AWS_ACCESS_KEY_ID=<access key id>
export AWS_SECRET_ACCESS_KEY=<secret>
export S3_ENDPOINT=https://storage.example.com   # пропустить для AWS S3
export S3_REGION=us-east-1
export S3_BUCKET_PREFIX=user-                     # bucket = user-<slug>
```

### Переменные окружения

| Переменная                | Обязательная | Описание                                                                  |
|---------------------------|:------------:|---------------------------------------------------------------------------|
| `AWS_ACCESS_KEY_ID`       | ✅            | Access key id                                                             |
| `AWS_SECRET_ACCESS_KEY`   | ✅            | Secret access key                                                         |
| `S3_ENDPOINT`             | —            | URL провайдера (пусто = AWS). Примеры ниже.                               |
| `S3_REGION`               | —            | Регион (по умолчанию `us-east-1`)                                         |
| `S3_BUCKET_PREFIX`        | —            | Префикс имени бакета (по умолчанию `user-`) → бакет: `<prefix><slug>`     |
| `S3_PUBLIC_URL_TEMPLATE`  | —            | Шаблон публичной ссылки, `{bucket}` заменяется на имя                     |

### Примеры endpoint'ов

| Провайдер                    | `S3_ENDPOINT`                                     |
|------------------------------|---------------------------------------------------|
| AWS S3                       | *(не задавать)*                                   |
| MinIO (локально)             | `http://localhost:9000`                           |
| Cloud.ru                     | `https://storage.clo.ru`                          |
| Yandex Object Storage        | `https://storage.yandexcloud.net`                 |
| DigitalOcean Spaces (NYC3)   | `https://nyc3.digitaloceanspaces.com`             |
| Backblaze B2 (us-west-002)   | `https://s3.us-west-002.backblazeb2.com`          |

---

## Быстрый старт

Запускать можно из корня проекта (пути ниже оттуда) или `cd storage/tools/s3-user-cli && ./s3-user.sh …`.

```bash
# создать бакет под юзера "alice" → <prefix>alice
./storage/tools/s3-user-cli/s3-user.sh create alice

# загрузить файл
./storage/tools/s3-user-cli/s3-user.sh put alice ./track.mp3 music/track.mp3

# presigned URL (TTL 1 час)
./storage/tools/s3-user-cli/s3-user.sh url alice music/track.mp3 3600

# бакет публичным (read-only для всех)
./storage/tools/s3-user-cli/s3-user.sh policy alice public

# разрешающий CORS
./storage/tools/s3-user-cli/s3-user.sh cors alice

# листинг
./storage/tools/s3-user-cli/s3-user.sh ls alice

# снести бакет со всем содержимым
./storage/tools/s3-user-cli/s3-user.sh delete alice --force
```

---

## Все команды

| Команда                            | Что делает                                                 |
|------------------------------------|------------------------------------------------------------|
| `create <user>`                    | Создать бакет `<prefix><slug>`                             |
| `delete <user> [--force]`          | Удалить бакет (с `--force` — сначала очистить)             |
| `list`                             | Список всех бакетов аккаунта                               |
| `ls <user> [prefix]`               | Листинг объектов (рекурсивно, human-readable)              |
| `info <user>`                      | Регион, размер, количество объектов                        |
| `put <user> <file> [key]`          | Загрузить файл (key по умолчанию = basename)               |
| `get <user> <key> [dest]`          | Скачать объект                                             |
| `rm <user> <key>`                  | Удалить объект                                             |
| `url <user> <key> [ttl_seconds]`   | Presigned URL (по умолчанию TTL 3600)                      |
| `policy <user> public\|private`    | Сделать бакет публичным / приватным                        |
| `cors <user>`                      | Применить разрешающий CORS (GET/HEAD/PUT, `*`)             |
| `name <user>`                      | Показать вычисленное имя бакета без создания               |
| `raw <args...>`                    | Пробросить произвольные аргументы в `aws` с нужным endpoint|

Встроенный help:
```bash
./storage/tools/s3-user-cli/s3-user.sh help
```

---

## Нейминг

`<user>` нормализуется: lowercase + всё, что не `[a-z0-9-]`, заменяется на `-`; затем добавляется `S3_BUCKET_PREFIX`.

Примеры (при `S3_BUCKET_PREFIX=user-`):

| Ввод     | Бакет          |
|----------|----------------|
| `alice`  | `user-alice`   |
| `User_42`| `user-user-42` |
| `Иван`   | ошибка (кириллицу S3-имена не поддерживают) |

---

## Ссылки на объекты

После `create alice`:

- **path-style:** `<S3_ENDPOINT>/<bucket>/<key>` (или `s3://<bucket>/<key>` для AWS)
- **public URL:** `S3_PUBLIC_URL_TEMPLATE` с подстановкой `{bucket}` — если задан

Для приватных бакетов — `url` для временной ссылки.

---

## Примеры `raw`

```bash
./storage/tools/s3-user-cli/s3-user.sh raw s3api list-buckets
./storage/tools/s3-user-cli/s3-user.sh raw s3api get-bucket-versioning --bucket user-alice
./storage/tools/s3-user-cli/s3-user.sh raw s3api put-bucket-versioning \
    --bucket user-alice \
    --versioning-configuration Status=Enabled
```

---

## Траблшутинг

- `InvalidAccessKeyId` — проверь `AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY`.
- `BucketAlreadyOwnedByYou` — бакет уже создан, ок.
- `BucketAlreadyExists` — имя занято глобально у провайдера, поменяй `<user>` или `S3_BUCKET_PREFIX`.
- `AccessDenied` при `policy public` — провайдер может запрещать public-ACL, уточни в его документации.
- `Could not connect to the endpoint URL` — проверь `S3_ENDPOINT` (схема, домен, порт).
