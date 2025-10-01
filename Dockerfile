FROM rust:1.90-bullseye AS builder
WORKDIR /build
ARG BIN_NAME=dockcron

COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    cargo build --release && \
    cp target/release/${BIN_NAME} /build/${BIN_NAME}

FROM gcr.io/distroless/cc-debian12:nonroot

WORKDIR /app

ARG BIN_NAME=dockcron
COPY --from=builder /build/${BIN_NAME} /app/${BIN_NAME}

ENV RUST_LOG=info \
    DOCKER_HOST=unix:///var/run/docker.sock \
    LABEL_PREFIXES=dockcron,ofelia,chadburn

USER 0:0

ENTRYPOINT ["/app/dockcron", "run"]