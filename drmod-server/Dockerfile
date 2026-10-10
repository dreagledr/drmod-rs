# ── Build stage ──
FROM rust:1.97-bookworm AS builder
WORKDIR /build
COPY drmod-protocol/ drmod-protocol/
COPY drmod-server/ drmod-server/

# The image ships only the two server crates, so the build context has no workspace
# root for their `version.workspace = true` to resolve against. Recreate a minimal
# one, taking the release version from the root [workspace.package] via MOD_VERSION.
ARG MOD_VERSION=0.0.0
RUN printf '[workspace]\nresolver = "3"\nmembers = ["drmod-protocol", "drmod-server"]\n\n[workspace.package]\nversion = "%s"\n' "$MOD_VERSION" > Cargo.toml

# The workspace root above makes the target dir /build/target, not /build/drmod-server/target.
ENV CARGO_TARGET_DIR=/build/target
RUN cargo build --release --target x86_64-unknown-linux-gnu --manifest-path drmod-server/Cargo.toml

# ── Runtime stage ──
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /build/target/x86_64-unknown-linux-gnu/release/drmod-server .
EXPOSE 5222/tcp 5222/udp 8080/tcp
CMD ["./drmod-server"]
