# AGENTS.md

Instructions for the rqbit Web UI.

This directory contains the React + TypeScript frontend used by rqbit.

Follow the repository root instructions in addition to this file.

Read any existing Web UI-specific documentation such as `CLAUDE.md` before substantial changes.

## Architecture

Use existing frontend architecture rather than creating parallel patterns.

Before adding:

- API helpers;
- stores;
- polling;
- dialogs;
- tables;
- notifications;
- formatting helpers

search for an existing implementation.

Keep responsibilities separated:

- API request/response types belong with API types.
- HTTP calls belong in the existing API client layer.
- Shared application state belongs in the existing state-management layer.
- Component-local UI state should remain local.

Do not call the backend directly from arbitrary components if an established API abstraction exists.

## TypeScript

Maintain strict typing.

Avoid:

- unnecessary `any`;
- type assertions used merely to silence errors;
- duplicating backend interfaces in multiple components;
- ignoring nullable/optional states.

When backend request or response shapes change, update the frontend API types and consumers in the same task.

## State

Use the project's existing shared-state conventions.

Do not introduce another state-management library.

Do not duplicate backend-derived state in multiple stores unless there is a clear synchronization strategy.

When updating a mutation:

- handle success;
- handle failure;
- keep cached state consistent;
- refresh/invalidate according to existing patterns.

## Polling

Torrent data may be updated frequently.

Use existing polling/update helpers.

Do not create independent component timers when the shared architecture already polls the same data.

Clean up subscriptions/timers appropriately.

## Large torrent lists

Preserve list/table performance.

Do not accidentally disable virtualization for large torrent collections.

Apply filters to the data before handing it to a virtualized list rather than rendering everything and hiding rows with CSS.

Avoid expensive calculations on every render when they can be memoized or derived elsewhere.

## Authentication

Frontend authentication behavior must match backend session behavior.

Do not:

- store passwords unnecessarily;
- expose authentication cookies to JavaScript unless explicitly required;
- assume HTTP 200 means the user remains authenticated forever.

Handle unauthorized/session-expired responses deliberately.

When changing authentication UX, test both:

- logged-in state;
- logged-out/expired state.

## Categories

Category UI behavior must remain consistent with backend automation semantics.

When modifying category UI, consider:

- create;
- select/filter;
- assign;
- unassign;
- delete;
- uncategorized torrents;
- refresh after mutation;
- errors returned by the backend.

Do not implement category assignment as an implicit filesystem move unless backend semantics explicitly support that feature.

## Styling

Follow the existing Tailwind/component patterns.

New UI should work in the themes already supported by the application.

Avoid duplicating long class lists where an existing component or helper should be reused.

Do not redesign unrelated UI as part of a small feature.

## Accessibility

Interactive controls should remain keyboard-accessible.

Use semantic controls when possible.

Provide labels or accessible names for icon-only buttons.

Do not use color as the only way to communicate critical state.

## Formatting

After modifying TypeScript or TSX, run the repository's formatter.

Typically:

```bash
npm run format
```

Use the repository's existing package scripts rather than inventing alternative formatter commands.

## Validation

For frontend changes, run the relevant available checks, such as:

```bash
npm test
npm run build
npm run format:check
```

Use the actual scripts present in `package.json`.

Do not claim the frontend builds or type-checks unless you ran the corresponding command.

## Definition of done

Before completing Web UI work:

- review the rendered-state logic;
- review loading/error/empty states;
- update API types if needed;
- run formatting;
- run relevant tests;
- run a build/type check when practical;
- ensure authentication behavior was not accidentally weakened;
- verify dark/light theme behavior if styling changed.

State which checks were actually run.
