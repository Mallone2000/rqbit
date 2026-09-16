# AGENTS.md

Instructions for coding agents working in this repository.

This repository is a fork of `ikatson/rqbit` with additional support for automated media stacks, including Servarr-compatible qBittorrent API behavior, automation metadata, authentication, and additional persistence behavior.

Read the existing implementation before making assumptions based on upstream rqbit or qBittorrent.

## Instruction precedence

Follow instructions in this order:

1. Explicit user instructions.
2. The nearest `AGENTS.md` governing the file being changed.
3. Parent `AGENTS.md` files, including this one.
4. Existing repository documentation and established code conventions.

More specific `AGENTS.md` files may add to or override general rules in this file for their subtree.

Do not ignore a child `AGENTS.md`.

## Repository map

Important areas:

- `crates/rqbit/`
  - CLI and application startup.

- `crates/librqbit/`
  - Main rqbit library.
  - Torrent sessions and lifecycle.
  - HTTP APIs.
  - Persistence.
  - Storage.
  - Automation integration.

- `crates/librqbit/src/http_api/`
  - Native HTTP API.
  - Authentication.
  - Web UI serving.
  - qBittorrent compatibility routes.

- `crates/librqbit/src/http_api/qbittorrent/`
  - qBittorrent Web API compatibility for Servarr clients.

- `crates/librqbit/src/session_persistence/`
  - Persistent session state.
  - JSON and PostgreSQL persistence implementations.

- `crates/librqbit/webui/`
  - React + TypeScript Web UI.

- `desktop/`
  - Tauri desktop application.

- `docker/`
  - Container and deployment configuration.

Relevant repository documentation may include:

- `README.md`
- `DEV-GUIDE.md`
- `AI_POLICY.md`
- nearby `CLAUDE.md` files

Read relevant documentation before making architectural changes.

## Core working rules

Before editing:

1. Inspect the relevant implementation.
2. Search for existing abstractions before introducing new ones.
3. Read the nearest `AGENTS.md`.
4. Identify existing tests covering the behavior.
5. Understand whether the behavior is fork-specific or inherited from upstream rqbit.

Keep changes focused.

Do not perform unrelated refactors unless they are required to complete the requested task safely.

Do not silently change:

- public APIs;
- persisted formats;
- authentication semantics;
- environment-variable behavior;
- CLI behavior;
- Servarr-visible behavior;
- qBittorrent-compatible responses.

If one of these must change, update tests and documentation as part of the same task.

## User intent

Implement the user's requested change completely when the request is clear.

Do not stop merely because implementation requires touching multiple files.

Do not expand the task into unrelated cleanup.

When requirements are ambiguous but a safe, conventional interpretation exists, prefer implementing that interpretation rather than blocking unnecessarily.

## Rust

Follow existing Rust style and architecture.

Prefer:

- existing repository abstractions;
- typed domain errors when callers need to distinguish error cases;
- explicit validation at API and persistence boundaries;
- narrow visibility;
- small, testable functions.

Avoid:

- unnecessary cloning;
- broad `unwrap()` or `expect()` in production paths;
- swallowing errors;
- global state where an existing session/service abstraction can own the state;
- large refactors mixed with behavioral fixes.

Do not introduce a dependency when the standard library or an existing workspace dependency solves the problem cleanly.

When introducing a dependency:

- justify why it is necessary;
- use workspace dependency conventions where appropriate;
- enable only required features;
- include `Cargo.lock` changes.

## Search and repository inspection

Prefer `rg` for repository and log searches.

Examples:

```bash
rg "SessionPersistenceStore" crates/
rg "qbittorrent" crates/librqbit/
rg "ERROR" /tmp/rqbit-log | tail -100
```

Do not dump huge logs or generated files into context.

## Build and validation

For Rust changes, run the narrowest useful checks while iterating.

Before completing substantial Rust work, run as many of these as practical:

```bash
cargo fmt --all -- --check
cargo check
cargo clippy --all-targets
cargo test --workspace
```

Targeted tests are encouraged during development:

```bash
cargo test -p librqbit
```

Run more specific test filters when appropriate.

Never claim:

- compilation succeeded;
- tests passed;
- formatting passed; or
- linting passed

unless the corresponding command was actually executed successfully.

If a command cannot be run or fails because of a pre-existing/environmental issue, state that explicitly in the final response.

## Testing

Behavioral changes require tests when reasonably testable.

Bug fixes should normally include a regression test.

Prefer testing externally observable behavior over implementation details.

Test both success and failure paths when changing:

- validation;
- authentication;
- HTTP behavior;
- persistence;
- lifecycle transitions;
- automation compatibility.

Do not weaken an assertion merely to make a failing test pass unless the old assertion is demonstrably incorrect.

## Persistence

Persistent state is a compatibility surface.

When changing persisted data:

- preserve existing stored sessions where practical;
- provide sensible defaults for newly added fields;
- consider all persistence implementations;
- test serialization/restoration;
- do not silently discard state.

More specific rules are defined in:

```text
crates/librqbit/src/session_persistence/AGENTS.md
```

## HTTP and security

Treat externally reachable HTTP APIs as security-sensitive.

Do not weaken:

- authentication;
- authorization;
- origin validation;
- credential handling;
- login throttling;
- cookie security;
- request validation

without an explicit requirement.

Never log:

- passwords;
- authorization headers;
- authentication cookies;
- database credentials;
- private tracker credentials/passkeys;
- complete magnet URLs when they may contain sensitive parameters.

More specific HTTP rules are defined in:

```text
crates/librqbit/src/http_api/AGENTS.md
```

## qBittorrent / Servarr compatibility

The qBittorrent compatibility API is an external compatibility surface used by automation software such as Sonarr and Radarr.

Do not assume qBittorrent-shaped behavior can be simplified merely because rqbit internally behaves differently.

Preserve compatible:

- routes;
- request encodings;
- field names;
- state strings;
- response structures;
- status codes;
- authentication behavior;
- category behavior;
- torrent visibility.

More specific rules are defined in:

```text
crates/librqbit/src/http_api/qbittorrent/AGENTS.md
```

## Web UI

Backend API changes that alter response/request shapes must update matching frontend types and callers where applicable.

For TypeScript/TSX changes, follow:

```text
crates/librqbit/webui/AGENTS.md
```

## Desktop

Desktop-specific work follows:

```text
desktop/AGENTS.md
```

Do not run expensive full desktop builds unnecessarily when a smaller validation command proves the requested change.

## Docker

Container and Servarr deployment changes follow:

```text
docker/AGENTS.md
```

Do not commit real credentials, API keys, tracker secrets, or host-specific secrets.

## Upstream compatibility

Avoid gratuitous divergence from upstream rqbit.

For generic BitTorrent behavior:

- follow existing rqbit architecture;
- keep fork-specific integration isolated when practical;
- avoid changing unrelated upstream semantics;
- prefer changes that remain easy to rebase.

If a change has a generic rqbit component and a Servarr-specific component, keep the two separable where practical.

## Comments and documentation

Comments should explain:

- compatibility constraints;
- security constraints;
- lifecycle subtleties;
- non-obvious reasoning.

Do not add comments that merely restate the code.

Update documentation when user-facing configuration or behavior changes.

## Generated files

Do not manually modify generated artifacts unless the repository explicitly expects them to be edited.

Do not commit:

- build output;
- temporary files;
- local credentials;
- editor state;
- debugging artifacts.

## Definition of done

Before completing a task:

1. Re-read the applicable `AGENTS.md` instructions.
2. Review the entire diff.
3. Remove debugging code and temporary changes.
4. Run relevant formatting.
5. Run compilation/type checking.
6. Run relevant tests.
7. Run linting when practical.
8. Check for accidental secrets.
9. Check for unrelated modifications.
10. Confirm documentation still matches behavior.

In the final response, summarize:

- what changed;
- important design decisions;
- tests/checks actually run;
- any checks not run or still failing.

Never state that work is verified if it was not actually verified.
