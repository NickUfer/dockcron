# Dockcron

A lightweight, label-driven cron runner for Docker containers. It discovers containers by label and runs their scheduled
commands, similar to Ofelia, but with a minimal Rust-based implementation.

## Features

- Cron-like schedules via container labels (@every|@cron)
- Select containers by label/selector
- 

## Quick Start

- Pull image: `docker pull ghcr.io/nickufer/dockcron:v0.2.0`
- Run (inspects all running containers): `docker run --rm ghcr.io/nickufer/dockcron:v0.2.0`

Pass a container label selector to scope which containers are scanned for jobs:

- Env: `CONTAINER_LABEL_SELECTOR=com.docker.compose.project=myproj`

## Configuration

TODO

## Use Case: Mailcow Ofelia Replacement

[mailcow-dockerized](https://github.com/mailcow/mailcow-dockerized) uses Ofelia to run scheduled tasks.
Ofelia has some issues with memory usage and leaks. Dockcron works as a low profile drop-in replacement for Ofelia.
In my tests Ofelia uses ~235MB memory after the first few job executions, whereas Dockcron constantly uses less than 5MB.

To use Dockcron it is best to create a `docker-compose.override.yml` file in the root of your mailcow-dockerized setup
besides the actual docker compose file. This will reduce interference as much as possible.

This is a ready to use override file:
```yml
services:
  ofelia-mailcow:
    image: ghcr.io/nickufer/dockcron:v0.2.0
    command: ''
    environment:
      - CONTAINER_LABEL_SELECTOR=com.docker.compose.project=${COMPOSE_PROJECT_NAME}
```

After saving you need to apply the changes with: `docker compose up -d`. With that the replacement is done.

This setup mirrors Ofelia’s role by scanning the Mailcow project containers via the label selector.

With `docker stats` you can view the memory usage of the ofelia container before and after the override.

Before override:

```
CONTAINER ID   NAME                               CPU %     MEM USAGE / LIMIT     MEM %     NET I/O           BLOCK I/O         PIDS
db287016a0b8   mailcow-ofelia-mailcow-1           0.00%     236.7MiB / 7.755GiB   2.98%     1.09kB / 1.4kB    0B / 0B           11
...
```

After override (still the same container name):

```
CONTAINER ID   NAME                               CPU %     MEM USAGE / LIMIT     MEM %     NET I/O           BLOCK I/O         PIDS
76ec13d0ea5c   mailcow-ofelia-mailcow-1           0.00%     2.586MiB / 1.827GiB   0.14%     4.85kB / 126B     1.96GB / 0B       3
...
```

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
