FROM rust:1.94-bookworm AS builder

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY templates ./templates

RUN cargo build --release

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates tzdata \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/gptmail /usr/local/bin/gptmail

RUN mkdir -p /data

ENV APP_BASE_URL=http://localhost:3000 \
    HTTP_BIND=0.0.0.0:3000 \
    SMTP_BIND=0.0.0.0:2525 \
    SQLITE_PATH=/data/gptmail.sqlite3 \
    RETENTION_DAYS=1 \
    DAILY_LIMIT=0 \
    TOTAL_LIMIT=0 \
    RUST_LOG=info

EXPOSE 3000 2525

CMD ["gptmail"]
