# AGENTS.md

Instructions for the qBittorrent Web API compatibility layer.

This code exists primarily to make rqbit interoperable with automation clients such as Sonarr and Radarr.

Compatibility behavior is more important here than internal elegance.

Follow the repository root and parent HTTP API instructions in addition to this file.

## Compatibility principle

Do not implement what qBittorrent "ought" to do.

Implement the behavior required by supported qBittorrent API consumers.

Before changing behavior, determine whether Sonarr/Radarr depend on:

- the route;
- the exact field name;
- request encoding;
- response shape;
- status code;
- state value;
- authentication flow;
- timing/visibility semantics.

Apparently strange behavior may exist intentionally for compatibility.

## Scope

This is a compatibility layer, not necessarily a complete qBittorrent clone.

Do not expand scope to unrelated qBittorrent functionality unless requested or required by supported clients.

Unsupported features should behave deliberately.

Do not return fake success when a client reasonably expects an observable effect unless an intentional compatibility no-op is already established.

## Authentication

Do not weaken qBittorrent API authentication.

Do not create anonymous compatibility routes merely for convenience.

When modifying login/session behavior, test:

- valid credentials;
- invalid credentials;
- missing credentials;
- logout/session invalidation where applicable;
- supported alternate authentication mechanisms.

Never log credentials or active session identifiers.

## Request formats

qBittorrent clients commonly use:

- query parameters;
- `application/x-www-form-urlencoded`;
- multipart forms.

Tests should use the same encoding real clients use for the route being tested.

Do not rewrite an endpoint to JSON-only unless compatibility explicitly allows it.

## Responses

Treat qBittorrent-facing DTOs as external contracts.

Do not expose internal rqbit Rust types directly when their representation differs from qBittorrent.

Be careful changing:

- field names;
- optional fields;
- nullability;
- numeric units;
- timestamp units;
- booleans represented as integers;
- state strings.

Use explicit translation between rqbit state and qBittorrent-compatible responses.

## Torrent states

qBittorrent torrent state strings are compatibility-sensitive.

Do not casually rename or consolidate states.

Mappings may include states such as:

- `metaDL`;
- `checkingDL`;
- `downloading`;
- `stalledDL`;
- `pausedDL`;
- `uploading`;
- `stalledUP`;
- `pausedUP`;
- `error`.

When modifying state mapping, add tests for the affected lifecycle conditions.

## Pending magnets

A magnet may need to remain visible to automation clients before torrent metadata has resolved.

Do not make an accepted magnet disappear from the API while metadata is being resolved.

Changes involving pending torrents must consider:

1. immediate API visibility;
2. info-hash identity;
3. assigned category;
4. deletion while pending;
5. category changes while pending;
6. failed metadata resolution;
7. successful transition to a managed torrent;
8. persistence where applicable.

Do not replace asynchronous pending behavior with blocking metadata resolution without explicitly evaluating Servarr compatibility.

## Categories

Categories are automation metadata.

Do not implicitly reinterpret a category as a filesystem move operation.

Unless explicitly requested as a separate feature, assigning a category must not automatically move torrent contents into a directory named after the category.

Expected category behavior:

- the empty category means uncategorized;
- categories can be created and removed;
- assigning a non-empty category should require a valid category according to existing behavior;
- category changes should survive persistence where supported;
- removing a category must not delete its torrents.

Pending torrents must remain consistent with category changes.

## Automation metadata

When adding or changing automation metadata, consider the complete lifecycle:

1. torrent submission;
2. pending metadata resolution;
3. downloading;
4. completion;
5. seeding;
6. pause/resume;
7. category changes;
8. process restart;
9. deletion/forgetting.

Do not update only the API DTO while forgetting underlying session/persistence behavior.

## Torrent deletion

Deletion behavior is highly visible to Servarr.

Before changing delete semantics, understand whether the request means:

- forget torrent;
- delete downloaded data;
- cancel pending resolution;
- remove automation metadata.

Make these operations consistent.

Avoid leaving orphaned automation metadata or pending state.

## Compatibility tests

Every behavioral change in this directory should normally include a route-level regression test.

Tests should assert observable compatibility behavior.

Prefer testing:

- exact HTTP method;
- realistic request encoding;
- status code;
- relevant response body;
- resulting torrent/session state.

When fixing a Sonarr/Radarr incompatibility, encode the failing request shape in a regression test whenever practical.

## No speculative qBittorrent behavior

If uncertain about qBittorrent's API semantics, verify them before implementing them.

Do not guess based solely on endpoint names.

If Servarr behavior and general qBittorrent behavior differ, preserve the behavior required by the supported integration unless the task explicitly changes that support policy.

## Definition of done

Before completing qBittorrent API work:

- run the relevant compatibility tests;
- run relevant `librqbit` tests;
- check that auth behavior remains protected;
- check pending magnets when lifecycle behavior changed;
- check category behavior when automation metadata changed;
- confirm persistence impact;
- review response compatibility.

State exactly what was tested.
