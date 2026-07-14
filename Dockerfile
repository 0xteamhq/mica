# syntax=docker/dockerfile:1.7

# ---- builder ----
# We stay on the dynamic-cc image because reqwest/bollard pull native-tls
# (libssl). M12 T52 originally targeted musl-static + distroless/static,
# but that requires switching the TLS stack to rustls — tracked for a
# future polish pass. Final image is still small (~37 MB) thanks to
# distroless/cc-debian12.
FROM rust:1.88-bookworm AS builder
WORKDIR /src

RUN apt-get update && apt-get install -y --no-install-recommends \
        pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY src ./src
# `wasmtime::component::bindgen!({ path: "wit" })` in src/plugins/mod.rs
# reads the WIT package at compile time — without it the build fails with
# "could not find `mica` in `bindings`".
COPY wit ./wit
# include_str!()'d at compile time by src/handlers/openapi.rs.
COPY deploy/openapi ./deploy/openapi

RUN cargo build --release --locked
RUN strip target/release/mica

# ---- runtime ----
FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=builder /src/target/release/mica /usr/local/bin/mica
USER nonroot
EXPOSE 4444
ENTRYPOINT ["/usr/local/bin/mica"]
