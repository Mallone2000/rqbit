import { useContext, type ReactNode } from "react";
import { APIContext } from "../context";
import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import type { PendingTorrent } from "../api-types";
import { PendingMetadata } from "./PendingMetadata";

const fixture: PendingTorrent = {
  info_hash: "0101010101010101010101010101010101010101",
  name: "Linux fixture",
  category: "sonarr",
  state: "resolving_metadata",
  metadata_attempts: 1,
  error_code: "metadata_timeout",
  message:
    "Metadata discovery timed out; retrying. This does not establish corruption.",
  next_retry_at_unix_seconds: 12345,
};

function WithActions({ children }: { children: ReactNode }) {
  const API = useContext(APIContext);
  return (
    <APIContext.Provider value={{ ...API, pendingAction: async () => {} }}>
      {children}
    </APIContext.Provider>
  );
}

describe("pending metadata diagnostics", () => {
  it("does not show an empty panel", () => {
    expect(renderToStaticMarkup(<PendingMetadata torrents={[]} />)).toBe("");
  });

  it("explains a timeout and the scheduled retry", () => {
    const html = renderToStaticMarkup(<PendingMetadata torrents={[fixture]} />);
    expect(html).toContain("Linux fixture");
    expect(html).toContain("sonarr");
    expect(html).toContain("Waiting for metadata");
    expect(html).toContain("does not establish corruption");
    expect(html).toContain("Attempts: 1");
    expect(html).toContain("Next retry:");
  });

  it("reports a fatal error without promising a retry and falls back to the hash", () => {
    const html = renderToStaticMarkup(
      <PendingMetadata
        torrents={[
          {
            ...fixture,
            name: null,
            state: "error",
            error_code: "storage_initialization_failed",
            message:
              "Could not initialize torrent files. Check free space and permissions.",
            next_retry_at_unix_seconds: null,
          },
        ]}
      />,
    );
    expect(html).toContain(fixture.info_hash);
    expect(html).toContain("Could not add torrent");
    expect(html).toContain("Check free space and permissions");
    expect(html).not.toContain("Next retry:");
  });
  it("offers pause and file-preserving removal while discovery is active", () => {
    const html = renderToStaticMarkup(
      <WithActions>
        <PendingMetadata torrents={[fixture]} />
      </WithActions>,
    );
    expect(html).toContain(">Pause</button>");
    expect(html).toContain("Remove, keep files");
  });

  it("shows paused discovery as stopped and offers resume", () => {
    const html = renderToStaticMarkup(
      <WithActions>
        <PendingMetadata
          torrents={[
            { ...fixture, state: "paused", next_retry_at_unix_seconds: null },
          ]}
        />
      </WithActions>,
    );
    expect(html).toContain("Paused while waiting for metadata");
    expect(html).toContain("stopped until you resume");
    expect(html).toContain(">Resume</button>");
    expect(html).not.toContain("retrying");
    expect(html).not.toContain("Next retry:");
  });

  it("allows retrying a fatal pending submission", () => {
    const html = renderToStaticMarkup(
      <WithActions>
        <PendingMetadata
          torrents={[
            { ...fixture, state: "error", next_retry_at_unix_seconds: null },
          ]}
        />
      </WithActions>,
    );
    expect(html).toContain(">Retry</button>");
  });
});
