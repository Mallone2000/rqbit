# AGENTS.md

Instructions for the Tauri desktop application.

The desktop app wraps/sharedly consumes rqbit Web UI functionality.

Follow the root `AGENTS.md` and applicable Web UI conventions.

## Scope

Keep desktop-specific behavior in `desktop/`.

Do not fork generic Web UI behavior into a desktop-only implementation unless platform integration requires it.

Changes that apply equally to browser and desktop users usually belong in the shared Web UI.

## Security

Treat Tauri/native capabilities as privileged.

Do not expose broader filesystem, shell, networking, or native capabilities than necessary.

When changing Tauri permissions/capabilities:

- grant the minimum required access;
- understand which frontend code can invoke it;
- avoid wildcard permissions where narrower rules suffice.

Do not place secrets in frontend-accessible configuration.

## Frontend

Follow the same TypeScript conventions as the shared Web UI where applicable.

Do not duplicate API types or business logic already provided by the shared frontend.

## Native integration

Keep Rust/native commands small and explicit.

Validate arguments crossing the frontend/native boundary.

Do not trust values merely because they originated in the application's frontend.

Return structured errors rather than panicking.

## Builds

Full Tauri builds can be expensive.

For ordinary TypeScript/frontend changes, use the narrowest useful validation first.

For example, use the existing TypeScript type-check command when available.

Run a full desktop build only when:

- native Rust changed;
- Tauri configuration changed;
- packaging changed;
- platform integration changed;
- the requested task specifically requires build verification.

## Cross-platform behavior

Do not assume Linux-only filesystem or shell behavior unless the feature is explicitly platform-specific.

Consider:

- Windows paths;
- macOS paths;
- Linux paths;
- URL handling;
- permissions.

Use existing Tauri/platform abstractions rather than shelling out when possible.

## Generated configuration

Do not manually edit generated Tauri/build artifacts unless documented as source-controlled configuration.

## Definition of done

For desktop changes:

- run formatting;
- run TypeScript checks for frontend changes;
- run Rust checks for native Rust changes;
- inspect Tauri permission changes carefully;
- build the desktop app when the affected layer requires it.

State clearly if a full desktop build was not run.
