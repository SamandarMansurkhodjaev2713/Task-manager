# Toolchain: keep >= `rust-version` in Cargo.toml (currently 1.85); image tracks stable.
FROM rust:1.91-bookworm AS builder

WORKDIR /app

ENV RUSTFLAGS=-Cdebuginfo=0

# Build-time metadata.  Populated via `docker build --build-arg GIT_SHA=...`
# (or by docker-compose build args).  Surfaced through the `/version`
# endpoint so deploys are cross-checkable.
ARG GIT_SHA=unknown
ENV GIT_SHA=${GIT_SHA}

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates libsqlite3-dev pkg-config \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock build.rs ./
# Migrations before `src/` so edits to application code do not invalidate this layer.
COPY migrations ./migrations
COPY src ./src
COPY tests ./tests

RUN RUSTC_VERSION=$(rustc --version) \
    && export RUSTC_VERSION \
    && cargo build --release

FROM rust:1.91-bookworm AS test-runner

WORKDIR /app

ENV RUSTFLAGS=-Cdebuginfo=0

ARG GIT_SHA=unknown
ENV GIT_SHA=${GIT_SHA}

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates libsqlite3-dev pkg-config \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock build.rs ./
COPY migrations ./migrations
COPY src ./src
COPY tests ./tests

# Run unit tests (library targets only; integration tests requiring env-vars run via docker-compose).
RUN RUSTC_VERSION=$(rustc --version) \
    && export RUSTC_VERSION \
    && cargo test --lib --workspace

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
