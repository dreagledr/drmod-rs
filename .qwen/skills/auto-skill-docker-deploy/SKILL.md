---
name: docker-deploy
description: Docker + docker-compose setup for a Rust server crate — multi-stage build with correct rustc version, cross-compile to Linux, minimal runtime, TCP+UDP port exposure
source: auto-skill
extracted_at: '2026-07-20T13:23:49.699Z'
---

# Docker deploy for Rust server crate

## When to use

Adding Docker + docker-compose to a Rust server crate that is a workspace member. The server compiles to a Linux binary for Docker, but the host may be Windows (or any OS).

## Approach

### 1. Detect the Rust version

Use `rustc --version` (or read `share/doc/rust/html/releases.md` from the toolchain) to get the exact stable version (e.g. `1.97.0`). Pin the Docker build image to that version: `rust:<major>.<minor>-bookworm`.

### 2. Multi-stage Dockerfile

```dockerfile
# ── Build stage ──
FROM rust:<version>-bookworm AS builder
WORKDIR /build
COPY . .
RUN cargo build --release --target x86_64-unknown-linux-gnu

# ── Runtime stage ──
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /build/target/x86_64-unknown-linux-gnu/release/<binary> .
EXPOSE <port>/tcp <port>/udp
CMD ["./<binary>"]
```

**Why:** `rust:…-bookworm` gives a full toolchain with `x86_64-unknown-linux-gnu` target pre-installed. `debian:bookworm-slim` keeps the runtime image small. `ca-certificates` is needed if the server makes HTTPS calls.

### 3. .dockerignore

```
target/
```

Prevents the local build artifacts from bloating the Docker context.

### 4. docker-compose.yml

```yaml
services:
  server:
    build: .
    ports:
      - "<port>:<port>/tcp"
      - "<port>:<port>/udp"
    restart: unless-stopped
```

Both TCP and UDP must be mapped explicitly — Docker's `ports:` only maps TCP by default.

### 5. Placement

Put all three files (`.dockerignore`, `Dockerfile`, `docker-compose.yml`) in the server crate directory (not the workspace root). The `COPY . .` in Dockerfile picks up `src/`, `Cargo.toml`, and `.cargo/config.toml` from that directory.

### 6. Verification

```bash
cd <server-crate-dir>
docker compose up --build
```

## How to apply

1. Check `rustc --version` on the host, pin the image tag accordingly
2. Read `Cargo.toml` for the binary name and any ports configured in `src/main.rs`
3. Create the three files as described
