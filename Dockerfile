FROM rust:1.91-bookworm

WORKDIR /app

ENV CARGO_BUILD_JOBS=1
ENV RUSTFLAGS=-Cdebuginfo=0

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY migrations ./migrations
COPY tests ./tests
COPY docker-entrypoint.sh /docker-entrypoint.sh

RUN chmod +x /docker-entrypoint.sh \
    && cargo build --release \
    && mkdir -p /app/data

ENV DATABASE_URL=sqlite:///app/data/app.db
ENV BIND_ADDRESS=0.0.0.0:8080

EXPOSE 8080

ENTRYPOINT ["/docker-entrypoint.sh"]
CMD ["/app/target/release/telegram-task-bot"]
