# Final runtime image: tiny, non-root
FROM gcr.io/distroless/cc-debian12:nonroot

WORKDIR /app

# Copy the prebuilt binary from host
# Build locally:
#   cargo build --release
# Then build image (optional custom name):
#   docker build --build-arg BIN_NAME=dockcron -t your/image .
ARG BIN_NAME=dockcron
COPY target/release/${BIN_NAME} /app/${BIN_NAME}

# ===== Environment reference =====
# DOCKER_HOST:
#   - unix:///var/run/docker.sock (common when mounting Docker socket)
#   - tcp://127.0.0.1:2375
#   - tcp://host:2376 (TLS via DOCKER_TLS_VERIFY/DOCKER_CERT_PATH)
# CONTAINER_LABEL_SELECTOR:
#   - Optional container-level selector (key=value[,key=value...]) if used by your setup.
# LABEL_PREFIXES:
#   - Comma-separated list of label prefixes to scan for jobs.
#   - Example: "ofelia,custom"
#   - For a given prefix P, container labels should include:
#       P.enabled=true
#       P.job-exec.<jobname>.schedule=<@cron|@every ...>
#       P.job-exec.<jobname>.command=<cmd>
#       P.job-exec.<jobname>.no-overlap=true|false (optional)
# =================================

# Sensible defaults; override at run time
ENV RUST_LOG=info \
    DOCKER_HOST=unix:///var/run/docker.sock \
    LABEL_PREFIXES=ofelia,chadburn

# Ensure Docker sends SIGTERM first, then SIGKILL after timeout
STOPSIGNAL SIGTERM

USER 0:0

# Run as PID 1 with exec form so it receives SIGINT/SIGTERM
ENTRYPOINT ["/app/dockcron", "run"]