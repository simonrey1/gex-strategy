# ── Stage 1: dependency cache (cargo-chef) ───────────────────────────────────
FROM rust:1-bookworm AS chef
RUN cargo install cargo-chef --locked
WORKDIR /build

FROM chef AS planner
COPY Cargo.toml Cargo.lock* ./
COPY src/ src/
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /build/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

COPY Cargo.toml Cargo.lock* ./
COPY src/ src/

RUN cargo build --release --bin live && \
    strip target/release/live && \
    cargo test --release export_bindings 2>&1 || true

# ── Stage 2: build dashboard ────────────────────────────────────────────────
FROM node:24-bookworm-slim AS dashboard

WORKDIR /app/dashboard

COPY dashboard/package.json dashboard/package-lock.json ./
RUN npm ci --ignore-scripts

COPY tsconfig.json ../tsconfig.json
COPY dashboard/tsconfig.json dashboard/vite.config.ts dashboard/index.html ./
COPY dashboard/src/ src/
COPY shared/ ../shared/
COPY --from=builder /build/bindings/ ../bindings/
RUN ln -s /app/dashboard/node_modules /app/node_modules

RUN npm run build

# ── Stage 3: production image ─────────────────────────────────────────────────
FROM debian:bookworm-slim

RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates curl && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /build/target/release/live ./live
COPY --from=dashboard /app/dashboard/dist/ dashboard/dist/

RUN mkdir -p data/downloads/live data/unified data/results data/live/history

EXPOSE 8080

ENTRYPOINT ["./live"]
CMD ["--port", "8080"]
