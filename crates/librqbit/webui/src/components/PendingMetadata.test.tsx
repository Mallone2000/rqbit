import { useContext, type ReactNode } from "react";
import { APIContext } from "../context";
import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import type { PendingTorrent } from "../api-types";
import { executePendingActions, PendingMetadata } from "./PendingMetadata";

const fixture: PendingTorrent = {
  info_hash: "0101010101010101010101010101010101010101",
  name: "Linux fixture",
  category: "sonarr",
  state: "resolving_metadata",
  metadata_attempts: 1,
  error_code: "metadata_peers_exhausted",
  message: "Available peers did not provide metadata; retrying discovery.",
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

  it("explains peer discovery and the scheduled retry", () => {
    const html = renderToStaticMarkup(<PendingMetadata torrents={[fixture]} />);
    expect(html).toContain("Linux fixture");
    expect(html).toContain("sonarr");
    expect(html).toContain("Waiting for metadata");
    expect(html).toContain("retrying discovery");
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

  it("offers selection and bulk actions for pending torrents", () => {
    const html = renderToStaticMarkup(
      <WithActions>
        <PendingMetadata
          torrents={[
            fixture,
            {
              ...fixture,
              info_hash: "0202020202020202020202020202020202020202",
              name: "Second fixture",
              state: "error",
            },
          ]}
        />
      </WithActions>,
    );
    expect(html).toContain("Select all");
    expect(html).toContain("Select Linux fixture");
    expect(html).toContain("Select Second fixture");
    expect(html).toContain("Pause selected (0)");
    expect(html).toContain("Resume / retry selected (0)");
    expect(html).toContain("Remove selected, keep files (0)");
    expect(html).toContain("disabled");
  });

  it("does not offer selection when pending actions are unavailable", () => {
    const html = renderToStaticMarkup(<PendingMetadata torrents={[fixture]} />);
    expect(html).not.toContain("Select all");
    expect(html).not.toContain("Select Linux fixture");
  });

  it("continues bulk actions after a failure and reports each result", async () => {
    const second = {
      ...fixture,
      info_hash: "0202020202020202020202020202020202020202",
      name: "Second fixture",
    };
    const calls: string[] = [];
    const result = await executePendingActions(
      [fixture, second],
      "forget",
      async (hash, action) => {
        calls.push(`${hash}:${action}`);
        if (hash === fixture.info_hash) throw new Error("request failed");
      },
    );
    expect(calls).toEqual([
      `${fixture.info_hash}:forget`,
      `${second.info_hash}:forget`,
    ]);
    expect(result).toEqual({
      succeeded: [second.info_hash],
      failed: ["Linux fixture: request failed"],
    });
  });

  it("shows the API's error text for a failed pending action", async () => {
    const result = await executePendingActions([fixture], "pause", async () => {
      throw { text: "Torrent was already removed" };
    });
    expect(result.failed).toEqual([
      "Linux fixture: Torrent was already removed",
    ]);
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
