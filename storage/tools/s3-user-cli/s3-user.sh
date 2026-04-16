#!/usr/bin/env bash
# Generic S3-compatible bucket management CLI.
# Works with any S3-compatible provider (AWS, MinIO, Cloud.ru, Yandex, etc.)
# Requires: aws-cli v1 or v2

set -euo pipefail

# ─── Config (all overridable via env) ──────────────────────────────────────
ENDPOINT="${S3_ENDPOINT:-}"                        # e.g. https://s3.amazonaws.com, https://storage.example.com
REGION="${S3_REGION:-us-east-1}"
BUCKET_PREFIX="${S3_BUCKET_PREFIX:-user-}"         # bucket name = <prefix><slug>
PUBLIC_URL_TEMPLATE="${S3_PUBLIC_URL_TEMPLATE:-}"  # optional, supports {bucket}; if unset — uses path-style on $ENDPOINT

: "${AWS_ACCESS_KEY_ID:?export AWS_ACCESS_KEY_ID}"
: "${AWS_SECRET_ACCESS_KEY:?export AWS_SECRET_ACCESS_KEY}"
export AWS_DEFAULT_REGION="$REGION"

AWS=(aws --region "$REGION")
[[ -n "$ENDPOINT" ]] && AWS+=(--endpoint-url "$ENDPOINT")
S3=("${AWS[@]}" s3)
S3API=("${AWS[@]}" s3api)

# ─── Helpers ───────────────────────────────────────────────────────────────
die()  { printf '\e[31merror:\e[0m %s\n' "$*" >&2; exit 1; }
info() { printf '\e[36m→\e[0m %s\n' "$*" >&2; }
ok()   { printf '\e[32m✓\e[0m %s\n' "$*" >&2; }

bucket_for_user() {
  local user="$1"
  [[ -n "$user" ]] || die "user is required"
  local slug
  slug=$(printf '%s' "$user" | tr '[:upper:]' '[:lower:]' | sed -E 's/[^a-z0-9-]+/-/g; s/^-+|-+$//g')
  [[ -n "$slug" ]] || die "user slug became empty after normalization (use latin letters/digits)"
  printf '%s%s' "$BUCKET_PREFIX" "$slug"
}

print_urls() {
  local bucket="$1"
  if [[ -n "$PUBLIC_URL_TEMPLATE" ]]; then
    printf 'public: %s\n' "${PUBLIC_URL_TEMPLATE//\{bucket\}/$bucket}"
  fi
  if [[ -n "$ENDPOINT" ]]; then
    printf 'path-style: %s/%s\n' "$ENDPOINT" "$bucket"
  else
    printf 'path-style: s3://%s\n' "$bucket"
  fi
}

usage() {
  cat <<EOF
Usage: $(basename "$0") <command> [args]

Commands:
  create   <user>                        Create bucket for user
  delete   <user> [--force]              Delete bucket (--force removes objects first)
  list                                   List all buckets
  ls       <user> [prefix]               List objects in user's bucket
  info     <user>                        Show bucket info (location, size, object count)
  put      <user> <file> [key]           Upload file
  get      <user> <key> [dest]           Download object
  rm       <user> <key>                  Delete object
  url      <user> <key> [ttl_seconds]    Generate presigned URL (default TTL 3600)
  policy   <user> public|private         Set bucket to public-read or private
  cors     <user>                        Apply permissive CORS (GET/HEAD/PUT)
  name     <user>                        Print computed bucket name for user
  raw      <args...>                     Run raw: aws <args> with configured endpoint

Env:
  AWS_ACCESS_KEY_ID          (required)
  AWS_SECRET_ACCESS_KEY      (required)
  S3_ENDPOINT                (default: AWS — leave empty; custom: https://storage.example.com)
  S3_REGION                  (default: us-east-1)
  S3_BUCKET_PREFIX           (default: user-)  — bucket name = <prefix><slug>
  S3_PUBLIC_URL_TEMPLATE     (optional) e.g. https://{bucket}.cdn.example.com
EOF
}

# ─── Commands ──────────────────────────────────────────────────────────────
cmd_create() {
  local user="${1:-}" bucket
  bucket=$(bucket_for_user "$user")
  info "creating bucket: $bucket"
  "${S3API[@]}" create-bucket --bucket "$bucket" >/dev/null
  ok "created: $bucket"
  print_urls "$bucket"
}

cmd_delete() {
  local user="${1:-}" force="${2:-}" bucket
  bucket=$(bucket_for_user "$user")
  if [[ "$force" == "--force" ]]; then
    info "emptying bucket: $bucket"
    "${S3[@]}" rm "s3://$bucket" --recursive >/dev/null || true
  fi
  info "deleting bucket: $bucket"
  "${S3API[@]}" delete-bucket --bucket "$bucket"
  ok "deleted: $bucket"
}

cmd_list() {
  "${S3[@]}" ls
}

cmd_ls() {
  local user="${1:-}" prefix="${2:-}" bucket
  bucket=$(bucket_for_user "$user")
  if [[ -n "$prefix" ]]; then
    "${S3[@]}" ls "s3://$bucket/$prefix" --recursive --human-readable --summarize
  else
    "${S3[@]}" ls "s3://$bucket" --recursive --human-readable --summarize
  fi
}

cmd_info() {
  local user="${1:-}" bucket
  bucket=$(bucket_for_user "$user")
  printf 'bucket: %s\n' "$bucket"
  printf 'location: '
  "${S3API[@]}" get-bucket-location --bucket "$bucket" --output text || true
  "${S3[@]}" ls "s3://$bucket" --recursive --summarize 2>/dev/null | tail -n 2 || true
}

cmd_put() {
  local user="${1:-}" file="${2:-}" key="${3:-}" bucket
  bucket=$(bucket_for_user "$user")
  [[ -f "$file" ]] || die "file not found: $file"
  [[ -z "$key" ]] && key=$(basename "$file")
  "${S3[@]}" cp "$file" "s3://$bucket/$key"
  ok "uploaded: s3://$bucket/$key"
}

cmd_get() {
  local user="${1:-}" key="${2:-}" dest="${3:-}" bucket
  bucket=$(bucket_for_user "$user")
  [[ -z "$dest" ]] && dest=$(basename "$key")
  "${S3[@]}" cp "s3://$bucket/$key" "$dest"
  ok "downloaded: $dest"
}

cmd_rm() {
  local user="${1:-}" key="${2:-}" bucket
  bucket=$(bucket_for_user "$user")
  "${S3[@]}" rm "s3://$bucket/$key"
}

cmd_url() {
  local user="${1:-}" key="${2:-}" ttl="${3:-3600}" bucket
  bucket=$(bucket_for_user "$user")
  "${S3[@]}" presign "s3://$bucket/$key" --expires-in "$ttl"
}

cmd_policy() {
  local user="${1:-}" mode="${2:-}" bucket
  bucket=$(bucket_for_user "$user")
  case "$mode" in
    public)
      local policy
      policy=$(cat <<JSON
{"Version":"2012-10-17","Statement":[{"Sid":"PublicRead","Effect":"Allow","Principal":"*","Action":["s3:GetObject"],"Resource":["arn:aws:s3:::$bucket/*"]}]}
JSON
)
      "${S3API[@]}" put-bucket-policy --bucket "$bucket" --policy "$policy"
      ok "bucket $bucket → public-read"
      ;;
    private)
      "${S3API[@]}" delete-bucket-policy --bucket "$bucket" 2>/dev/null || true
      ok "bucket $bucket → private"
      ;;
    *) die "policy mode must be 'public' or 'private'";;
  esac
}

cmd_cors() {
  local user="${1:-}" bucket
  bucket=$(bucket_for_user "$user")
  local cors
  cors=$(cat <<'JSON'
{"CORSRules":[{"AllowedOrigins":["*"],"AllowedMethods":["GET","HEAD","PUT"],"AllowedHeaders":["*"],"ExposeHeaders":["ETag","Content-Length","Content-Type"],"MaxAgeSeconds":3000}]}
JSON
)
  "${S3API[@]}" put-bucket-cors --bucket "$bucket" --cors-configuration "$cors"
  ok "CORS applied to $bucket"
}

cmd_name() {
  bucket_for_user "${1:-}"
}

cmd_raw() {
  "${AWS[@]}" "$@"
}

# ─── Dispatch ──────────────────────────────────────────────────────────────
cmd="${1:-}"; shift || true
case "$cmd" in
  create) cmd_create "$@";;
  delete) cmd_delete "$@";;
  list)   cmd_list "$@";;
  ls)     cmd_ls "$@";;
  info)   cmd_info "$@";;
  put)    cmd_put "$@";;
  get)    cmd_get "$@";;
  rm)     cmd_rm "$@";;
  url)    cmd_url "$@";;
  policy) cmd_policy "$@";;
  cors)   cmd_cors "$@";;
  name)   cmd_name "$@";;
  raw)    cmd_raw "$@";;
  ""|-h|--help|help) usage;;
  *) usage; die "unknown command: $cmd";;
esac
