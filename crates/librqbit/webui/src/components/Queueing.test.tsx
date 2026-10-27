import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { useContext } from "react";
import { APIContext } from "../context";
import { QueueActions } from "./buttons/QueueActions";
import { QueueingTab } from "./config/QueueingTab";
import { PendingMetadata } from "./PendingMetadata";
import { getSortValue, matchesStatus } from "../helper/torrentFilters";
import type { QueueConfig, TorrentListItem } from "../api-types";

const config: QueueConfig = {
  enabled: true,
  max_active_downloads: 3,
  max_active_uploads: 3,
  max_active_torrents: 5,
  dont_count_slow_torrents: false,
  slow_torrent_dl_rate_threshold: 2,
  slow_torrent_ul_rate_threshold: 2,
  slow_torrent_inactive_timer: 60,
};
function Controls() {
  const api = useContext(APIContext);
  return (
    <APIContext.Provider
      value={{
        ...api,
        reorderQueue: async () => {},
        setForceStart: async () => {},
      }}
    >
      <QueueActions hash={"ab".repeat(20)} position={2} forced={true} />
    </APIContext.Provider>
  );
}
describe("queue controls", () => {
  it("labels every setting, its units and unlimited behavior", () => {
    const html = renderToStaticMarkup(
      <QueueingTab config={config} onChange={() => {}} />,
    );
    for (const label of [
      "Active downloads",
      "Active seeds",
      "Total active torrents",
      "KiB/s",
      "Startup grace (seconds)",
      "-1 means unlimited",
      "Manual pauses remain stopped",
    ])
      expect(html).toContain(label);
    expect((html.match(/type="number"/g) ?? []).length).toBe(6);
  });
  it("renders semantic keyboard-accessible reorder and force buttons", () => {
    const html = renderToStaticMarkup(<Controls />);
    expect(html).toContain('aria-pressed="true"');
    for (const label of ["Top", "Up", "Down", "Bottom", "Clear force start"])
      expect(html).toContain(`>${label}</button>`);
    expect(html).not.toContain("<a ");
  });
  it("capability gates unsupported adapters", () => {
    expect(renderToStaticMarkup(<QueueActions hash="abc" position={1} />)).toBe(
      "",
    );
  });
  it("distinguishes queued downloads from user pauses and sorts positions", () => {
    const torrent: TorrentListItem = {
      id: 1,
      info_hash: "ab",
      name: "fixture",
      output_folder: "",
      total_pieces: 1,
      stats: {
        state: "paused",
        queued: true,
        queue_position: 2,
        error: null,
        file_progress: [],
        progress_bytes: 0,
        total_bytes: 1,
        finished: false,
        live: null,
      },
    };
    expect(matchesStatus(torrent, "queued")).toBe(true);
    expect(matchesStatus(torrent, "paused")).toBe(false);
    expect(getSortValue(torrent, "queue")).toBe(2);
  });
  it("describes a queued magnet without claiming active discovery", () => {
    const html = renderToStaticMarkup(
      <PendingMetadata
        torrents={[
          {
            info_hash: "ab",
            name: "fixture",
            category: "",
            state: "queued",
            metadata_attempts: 0,
            error_code: null,
            message: null,
            next_retry_at_unix_seconds: null,
          },
        ]}
      />,
    );
    expect(html).toContain("Queued for metadata");
    expect(html).not.toContain("Discovering peers");
  });
});
