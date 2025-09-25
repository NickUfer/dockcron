# Dockcron

A lightweight, label-driven cron runner for Docker containers. It discovers containers by label and runs their scheduled
commands, similar to Ofelia, but with a minimal Rust-based implementation.

## Features

- Cron-like schedules via container labels (@every|@cron)
- Select containers by label/selector
- 

## Quick Start

- Pull image: `docker pull ghcr.io/nickufer/dockcron:v0.1.0`
- Run (inspects all running containers): `docker run --rm ghcr.io/nickufer/dockcron:v0.1.0`

Pass a container label selector to scope which containers are scanned for jobs:

- Env: `CONTAINER_LABEL_SELECTOR=com.docker.compose.project=myproj`

## Configuration

TODO

## Use Case: Mailcow Ofelia Replacement

[mailcow-dockerized](https://github.com/mailcow/mailcow-dockerized) uses Ofelia to run scheduled tasks.
Ofelia has some issues with memory usage and leaks. Dockcron works as a low profile drop-in replacement for Ofelia.
Just drop a `docker-compose.override.yml` file into the root of your mailcow-dockerized setup.
This will run Dockcron and make sure it only discovers jobs from the current compose project:

```yml
services:
  ofelia-mailcow:
    image: ghcr.io/nickufer/dockcron:v0.1.0
    command: ''
    environment:
      - CONTAINER_LABEL_SELECTOR=com.docker.compose.project=${COMPOSE_PROJECT_NAME}
```

Apply changes: `docker compose up -d`.

This setup mirrors Ofelia’s role by scanning the Mailcow project containers via the label selector.

## Building from Source

- Prereqs: Rust 1.90.0, Cargo
- Build: `cargo build --release`
- Run: `./target/release/dockcron`

Docker image:

- Build: `docker build -t ghcr.io/nickufer/dockcron:local .`
- Run: `docker run --rm -e TZ=UTC ghcr.io/nickufer/dockcron:local`

## Logging

Logs are printed to stdout. Use your orchestrator or `docker logs` to collect/view. Set TZ for consistent timestamps.

## License

MIT
