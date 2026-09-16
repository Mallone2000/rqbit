# AGENTS.md

Instructions for session persistence code.

Persistent session state is a long-lived compatibility boundary.

A change that works before restart but loses state after restart is not complete.

Follow the repository root instructions in addition to this file.

## Persistence implementations

When changing shared persistent state, inspect every implementation of the session persistence abstraction.

Do not update only one backend unless the feature is explicitly backend-specific.

If this fork supports JSON/file persistence and PostgreSQL persistence for the affected state, both must remain semantically consistent.

## Shared contract

Treat the persistence trait/interface as the contract.

Do not put backend-specific semantics into callers unless necessary.

A caller should not need to know which persistence backend is in use for ordinary operations.

## New persisted fields

When introducing a new persisted field:

1. define its lifecycle;
2. choose a backward-compatible default;
3. update serialization;
4. update deserialization;
5. update every backend;
6. update database storage/schema behavior if applicable;
7. test loading old data where practical;
8. test round-trip persistence;
9. test restoration behavior.

Do not make previously valid persisted sessions unreadable without an explicit migration plan.

## Backward compatibility

Prefer additive persisted-format changes.

New fields should normally have sensible defaults when absent.

Do not rename or remove persisted fields casually.

If a breaking migration is unavoidable, document it clearly and add explicit migration handling where practical.

## Automation metadata

Persistent automation metadata may include:

- categories;
- limits;
- timestamps;
- completion state;
- activity state;
- upload/download accounting;
- Servarr-related metadata.

When modifying automation metadata, verify behavior across:

- pending torrents;
- managed torrents;
- process restart;
- deletion;
- category removal or reassignment.

Do not persist only the final managed torrent if automation requires pending state to survive or remain observable.

## Atomicity and consistency

Avoid states where memory reports that a durable change succeeded when persistence failed.

Consider operation ordering carefully.

For mutations involving both memory and persistence:

- persist first where safe; or
- roll back in-memory state after persistence failure; or
- use an existing transactional mechanism.

Never silently ignore persistence errors.

## PostgreSQL

For PostgreSQL changes:

- use existing query/migration conventions;
- bind parameters rather than interpolating values;
- avoid SQL injection;
- preserve transactions where multiple writes form one logical operation;
- consider concurrent updates.

Do not put secrets into logs.

## JSON/file persistence

File persistence should avoid corrupting the previously valid state when a write fails.

Use existing atomic-write conventions.

Do not replace safe write/rename behavior with direct destructive truncation unless there is a strong reason.

## Deletion

When a torrent/session is removed, determine which persisted data must also be removed.

Do not leave stale:

- categories;
- pending records;
- automation metadata;
- torrent-specific limits

unless intentionally retained.

## Testing

Persistence changes should normally include tests for:

- writing state;
- reading it back;
- defaults for missing fields;
- deletion where relevant;
- error propagation.

When behavior exists in multiple backends, test the shared contract rather than relying only on implementation-specific tests.

## Definition of done

Before completing persistence work:

- inspect all persistence implementations;
- confirm backward compatibility;
- verify serialization/deserialization;
- verify automation metadata lifecycle;
- run persistence tests;
- run relevant `librqbit` tests;
- run normal Rust formatting/checks.

Do not state that restart behavior works unless it was tested or directly covered by an equivalent persistence round trip.
