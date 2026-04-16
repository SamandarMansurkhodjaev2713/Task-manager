FROM rust:1.91-bookworm AS builder

WORKDIR /app

ENV CARGO_BUILD_JOBS=1
ENV RUSTFLAGS=-Cdebuginfo=0

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates libsqlite3-dev pkg-config \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY migrations ./migrations
COPY tests ./tests

RUN cargo build --release

FROM rust:1.91-bookworm AS test-runner

WORKDIR /app

ENV CARGO_BUILD_JOBS=1
ENV RUSTFLAGS=-Cdebuginfo=0

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates libsqlite3-dev pkg-config \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY migrations ./migrations
COPY tests ./tests

FROM debian:bookworm-slim AS runtime

WORKDIR /app

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl gosu libsqlite3-0 \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --system --gid 10001 taskbot \
    && useradd --system --uid 10001 --gid 10001 --create-home --home-dir /home/taskbot taskbot \
    && mkdir -p /app/data \
    && chown -R taskbot:taskbot /app /home/taskbot

COPY --from=builder /app/target/release/telegram-task-bot /usr/local/bin/telegram-task-bot
COPY docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh

RUN chmod +x /usr/local/bin/docker-entrypoint.sh

ENV DATABASE_URL=sqlite:///app/data/app.db
ENV BIND_ADDRESS=0.0.0.0:8080

EXPOSE 8080

HEALTHCHECK --interval=30s --timeout=5s --start-period=15s --retries=5 \
  CMD curl -fsS http://127.0.0.1:8080/healthz || exit 1

ENTRYPOINT ["/usr/local/bin/docker-entrypoint.sh"]
CMD ["telegram-task-bot"]
