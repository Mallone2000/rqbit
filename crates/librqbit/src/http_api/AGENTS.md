# AGENTS.md

Instructions for code under `crates/librqbit/src/http_api/`.

The HTTP layer is an externally visible and security-sensitive API boundary.

In addition to the repository root `AGENTS.md`, follow these rules.

## API compatibility

Treat existing HTTP routes as public contracts.

Before changing a route, inspect:

- callers;
- frontend usage;
- tests;
- compatibility APIs;
- authentication middleware.

Do not silently change:

- route paths;
- HTTP methods;
- request encoding;
- response JSON;
- status codes;
- authentication requirements.

If a contract must change, update all known consumers and tests.

## Request validation

Validate untrusted input at the HTTP boundary whenever practical.

Do not rely on downstream code to reject obviously invalid:

- identifiers;
- paths;
- category names;
- numeric limits;
- request bodies;
- query parameters.

Return intentional errors rather than panicking.

## Authentication

Authentication and authorization behavior is security-sensitive.

Do not:

- disable authentication to simplify development;
- introduce an unauthenticated mutation path;
- treat arbitrary localhost callers as trusted;
- bypass authentication middleware in a new route;
- leak credential validity through unnecessary detail.

Test authenticated and unauthenticated behavior for protected routes.

## Browser requests

Preserve existing browser-origin and same-origin protections.

Localhost alone is not a sufficient trust boundary: unrelated local applications can issue HTTP requests.

Do not broaden allowed origins without understanding the security implications.

## Credentials

Never log or expose:

- passwords;
- `Authorization` headers;
- auth cookies;
- session identifiers;
- connection strings containing credentials.

Avoid logging complete request headers.

Avoid logging full URIs when query strings may contain sensitive torrent/tracker information.

## Errors

External errors should be useful without exposing sensitive internals.

Prefer:

- stable status codes;
- typed internal errors;
- sanitized external messages.

Do not expose stack traces or credential-bearing backend errors to API clients.

## New routes

When adding an endpoint:

1. Place it in the correct API namespace.
2. Apply the correct auth middleware.
3. Validate request input.
4. Add tests for success.
5. Add tests for malformed input.
6. Add an unauthorized test when protected.
7. Update frontend/API types if consumed there.

## qBittorrent compatibility

Anything under:

`http_api/qbittorrent/`

also follows:

`http_api/qbittorrent/AGENTS.md`

The deeper file takes precedence for qBittorrent compatibility behavior.

## Validation

For HTTP changes, run targeted tests first.

Before finishing, run relevant `librqbit` tests and normal repository Rust checks required by the root `AGENTS.md`.

Do not claim compatibility or security behavior was verified without running the relevant tests.
