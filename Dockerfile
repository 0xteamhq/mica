# syntax=docker/dockerfile:1.7

# ---- builder ----
FROM rust:1.88-bookworm AS builder
WORKDIR /src

# Install build deps for native-tls (reqwest / bollard pull libssl).
RUN apt-get update && apt-get install -y --no-install-recommends \
        pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo build --release --locked
RUN strip target/release/mica

# ---- runtime ----
# distroless/cc has libc + libssl + CA bundle + nothing else.
FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=builder /src/target/release/mica /usr/local/bin/mica
USER nonroot
EXPOSE 4444
ENTRYPOINT ["/usr/local/bin/mica"]
