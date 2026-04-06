# GPTMail-compatible catch-all mail backend

A self-hosted temporary mailbox backend written in Rust.

## Features

- GPTMail-compatible core REST API
- Single-domain catch-all mailbox model
- Built-in SMTP receiver
- SQLite persistence
- Minimal Web UI at `/`, `/{email}`, `/{email}/{id}`
- Configurable retention window from 1 to 7 days

## Compatibility target

This project follows the documented API contract from:

- https://mail.chatgpt.org.uk/zh/api

Implemented v1 routes:

- `GET /api/generate-email`
- `POST /api/generate-email`
- `GET /api/emails?email=...`
- `GET /api/email/{id}`
- `DELETE /api/email/{id}`
- `DELETE /api/emails/clear?email=...`
- `GET /api/stats`

## Quick start

1. Copy `.env.example` to `.env`
2. Set `MAIL_DOMAIN` to your catch-all domain
3. Point MX for that domain at this server
4. Run `cargo run`

HTTP serves on `HTTP_BIND`, SMTP listens on `SMTP_BIND`.

## Notes

- For production catch-all, your MX record must point to this host and inbound SMTP must be reachable.
- If you need privileged port 25 in production, set `SMTP_BIND=0.0.0.0:25`.
- The Web UI stores the API key in browser localStorage because this project intentionally avoids a user account system.

## Docker deployment

Production compose files pull the prebuilt image directly:

- `rockxsj/gptmail:latest`

### 1. Prepare `.env`

Use `.env.example` as the base, then update at least:

```env
APP_BASE_URL=https://your-mail-host.example.com
MAIL_DOMAIN=mail.example.com
API_KEYS=your-secret-api-key
RETENTION_DAYS=1
HTTP_PORT=3000
SMTP_PORT=25
```

Notes:

- `MAIL_DOMAIN` is the catch-all domain.
- `SMTP_PORT=25` maps host port `25` to container port `2525`.
- Inside Docker, the app always listens on `0.0.0.0:3000` and `0.0.0.0:2525`.
- SQLite data is persisted to `./data/gptmail.sqlite3`.

### 2. Start

```bash
docker compose up -d
```

### 3. Check logs

```bash
docker compose logs -f
```

### 4. Stop / upgrade

```bash
docker compose pull
docker compose up -d
```

## Local rebuild override

If you want to build from the current source instead of pulling the published image, use the build override:

```bash
docker compose -f docker-compose.yml -f docker-compose.build.yml up -d --build
```

This keeps production deployment image-based while still allowing local source builds.

## Production checklist

- Point the **MX record** of `MAIL_DOMAIN` to this server.
- Open firewall ports:
  - `25/tcp` for SMTP
  - `3000/tcp` or put HTTP behind Nginx/Caddy
- If you deploy behind a reverse proxy, set `APP_BASE_URL` to the public URL.
- Keep `API_KEYS` private; the current UI stores the chosen key in browser localStorage.

## Docker + Nginx

If you want Nginx in front of the Rust app, use:

```bash
docker compose -f docker-compose.nginx.yml up -d
```

Recommended `.env` values for this mode:

```env
APP_BASE_URL=http://mail.example.com
MAIL_DOMAIN=mail.example.com
API_KEYS=your-secret-api-key
SMTP_PORT=25
HTTP_PUBLIC_PORT=80
NGINX_SERVER_NAME=mail.example.com
```

Behavior in this mode:

- Nginx publishes `80/tcp`
- GPTMail container is only exposed internally for HTTP
- SMTP still goes directly to the Rust app on host port `25 -> container 2525`

If you want to locally build the app image while using Nginx:

```bash
docker compose -f docker-compose.nginx.yml -f docker-compose.build.yml up -d --build
```

So the traffic path is:

- Web/API: `client -> nginx -> gptmail:3000`
- SMTP: `sender MTA -> gptmail:2525`

### Important note

Nginx here is only for **HTTP reverse proxy**.
It does **not** terminate SMTP. Your MX record must still point to the same host, and host port `25` must remain open for direct delivery to the GPTMail container.
