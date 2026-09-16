# AGENTS.md

Instructions for Docker, Compose, and deployment configuration.

These files are user-facing deployment interfaces.

Follow the repository root instructions in addition to this file.

## General rules

Keep examples runnable and internally consistent.

When changing an environment variable, port, mount path, or service name, search the repository for all related documentation and examples.

Do not update only one Compose example while leaving documentation inconsistent.

## Secrets

Never commit real:

- usernames/passwords;
- API keys;
- tracker credentials;
- private keys;
- VPN credentials;
- database passwords.

Use clearly fake placeholders and environment variables.

Do not include usable credentials even in commented examples.

## rqbit fork features

Do not assume an upstream `ikatson/rqbit` image contains fork-specific functionality.

Deployment examples that rely on fork-specific Servarr/qBittorrent compatibility must use an image/build that actually contains those features.

Keep image references deliberate.

## Authentication

Do not disable rqbit/qBittorrent-compatible API authentication merely to make a Compose example easier.

Examples should demonstrate secure credentials/configuration.

Avoid exposing writable management APIs publicly without authentication.

## Networking

Be deliberate about:

- published ports;
- container-only ports;
- bind addresses;
- reverse proxies;
- VPN/network namespaces.

Do not expose a port on the host unless users need host-level access.

Prefer internal Docker networking for communication between services where appropriate.

## Servarr integration

Sonarr/Radarr and rqbit must see compatible download paths.

When changing volumes or example paths, consider:

- container path consistency;
- completed download paths;
- media-import paths;
- hardlinks;
- filesystem boundaries.

Do not design an example that forces unnecessary copies when a shared filesystem layout can support hardlinks.

## Categories

Servarr categories are automation metadata.

Do not imply that assigning categories automatically changes rqbit filesystem paths unless that feature is explicitly implemented.

Document path behavior separately from category behavior.

## Persistent state

Mount rqbit state/configuration directories intentionally.

Do not create examples where container recreation unexpectedly destroys persistent session state.

Database-backed examples should include correct dependency/readiness behavior when appropriate.

## Compose

Use modern Compose syntax consistent with the repository.

Avoid unnecessary:

- privileged mode;
- host networking;
- broad device access;
- host filesystem mounts.

When additional privileges are required, document why.

## Health checks

Health checks should test actual application readiness when practical.

Do not make another service depend on rqbit solely because its container process exists if it requires the HTTP API to be ready.

## Documentation

When configuration behavior changes, update examples and user-facing documentation together.

Examples should explain:

- required environment variables;
- required credentials;
- relevant ports;
- storage mounts;
- Servarr configuration assumptions.

## Validation

After modifying Compose files, validate them with the available Docker Compose tooling when possible, for example:

```bash
docker compose config
```

Build affected images when Dockerfile behavior changes and the environment permits it.

Do not claim containers were successfully started unless they were actually started.

## Definition of done

Before completing Docker/deployment work:

- validate Compose syntax;
- check environment-variable names;
- check mounts;
- check port exposure;
- check authentication;
- check for secrets;
- verify documentation matches the example;
- review Servarr path consistency when applicable.

State which deployment checks were actually run.
