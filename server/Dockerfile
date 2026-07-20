# ── Build stage ──
FROM rust:1.97-bookworm AS builder
WORKDIR /build
COPY . .
RUN cargo build --release --target x86_64-unknown-linux-gnu

# ── Runtime stage ──
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /build/target/x86_64-unknown-linux-gnu/release/drmod-server .
EXPOSE 5222/tcp 5222/udp
CMD ["./drmod-server"]
