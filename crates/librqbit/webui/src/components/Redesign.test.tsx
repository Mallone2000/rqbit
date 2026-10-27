import { describe, expect, it, beforeEach } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { useContext } from "react";
import { summarizeTorrents } from "../helper/torrentSummary";
import { torrentStatusLabel } from "../helper/torrentStatusLabel";
import { TorrentListItem, TorrentStats } from "../api-types";
import { IconButton } from "./buttons/IconButton";
import { Button } from "./buttons/Button";
import { LoginForm, loginErrorMessage } from "./LoginForm";
import { ProgressBar } from "./ProgressBar";
import { OverviewTab } from "./compact/OverviewTab";
import { APIContext } from "../context";
import { SessionSummary, SessionSummaryView } from "./SessionSummary";
import { useStatsStore } from "../stores/statsStore";
import { useTorrentStore } from "../stores/torrentStore";

const stats = (overrides: Partial<TorrentStats> = {}): TorrentStats => ({
  state: "paused",
  error: null,
  file_progress: [],
  progress_bytes: 512,
  total_bytes: 1024,
  finished: false,
  live: null,
  ...overrides,
});
const torrent = (overrides: Partial<TorrentStats> = {}): TorrentListItem => ({
  id: 42,
  info_hash: "ab".repeat(20),
  name: "Example torrent",
  output_folder: "/downloads",
  total_pieces: 4,
  stats: stats(overrides),
});

describe("accessible workspace", () => {
  beforeEach(() => {
    useStatsStore.setState({ loaded: false });
    useTorrentStore.setState({ torrents: null });
  });
  it("keeps queued torrents distinct from pauses and counts completed independently", () => {
    expect(
      summarizeTorrents([
        torrent({ state: "live" }),
        torrent({ state: "live", finished: true }),
        torrent({ queued: true }),
        torrent(),
        torrent({ finished: true }),
      ]),
    ).toEqual({ active: 2, queued: 1, paused: 2, completed: 2 });
  });
  it("distinguishes missing data from an empty session", () => {
    expect(summarizeTorrents(null)).toBeNull();
    expect(summarizeTorrents([{ ...torrent(), stats: undefined }])).toBeNull();
    expect(summarizeTorrents([])).toEqual({
      active: 0,
      queued: 0,
      paused: 0,
      completed: 0,
    });
    const html = renderToStaticMarkup(<SessionSummary />);
    expect(html).toContain("—");
    expect(html).not.toContain("N/A");
  });
  it("uses the complete torrent store for summary totals", () => {
    useTorrentStore.setState({
      torrents: [torrent(), torrent({ queued: true })],
    });
    const html = renderToStaticMarkup(
      <SessionSummaryView
        stats={useStatsStore.getState().stats}
        loaded={false}
        torrents={useTorrentStore.getState().torrents}
      />,
    );
    expect(html).toContain("Paused");
    expect(html).toContain("Queued");
    expect(html).toContain("Includes seeds and pauses");
    expect(html).toContain(">1</div>");
  });
  it("provides text for checking, queue, error, missing and completed states", () => {
    expect(torrentStatusLabel()).toBe("Awaiting stats");
    expect(torrentStatusLabel(stats({ queued: true, finished: true }))).toBe(
      "Queued for seeding",
    );
    expect(torrentStatusLabel(stats({ state: "initializing" }))).toBe(
      "Checking files",
    );
    expect(torrentStatusLabel(stats({ state: "error" }))).toBe("Error");
    expect(torrentStatusLabel(stats({ state: "live", finished: true }))).toBe(
      "Seeding",
    );
  });
  it("uses named, natively disabled buttons for actions and anchors for navigation", () => {
    const html = renderToStaticMarkup(
      <IconButton title="Pause torrent" disabled onClick={() => {}}>
        Pause
      </IconButton>,
    );
    expect(html).toContain(
      '<button type="button" disabled="" aria-label="Pause torrent"',
    );
    expect(html).not.toContain("<a");
    expect(
      renderToStaticMarkup(
        <IconButton title="Open playlist" href="/playlist" onClick={() => {}}>
          Open
        </IconButton>,
      ),
    ).toContain('href="/playlist"');
    expect(
      renderToStaticMarkup(
        <Button disabled onClick={() => {}}>
          Delete
        </Button>,
      ),
    ).toContain('disabled=""');
  });
  it("retains ID, queue actions and transfer totals in Overview", () => {
    function Overview() {
      const api = useContext(APIContext);
      return (
        <APIContext.Provider
          value={{
            ...api,
            getPlaylistUrl: () => null,
            reorderQueue: async () => {},
            setForceStart: async () => {},
          }}
        >
          <OverviewTab
            torrent={torrent({ queue_position: 3, force_start: true })}
          />
        </APIContext.Provider>
      );
    }
    const html = renderToStaticMarkup(<Overview />);
    for (const text of [
      "ID",
      "42",
      "Queue position",
      "Received",
      "Sent",
      "Move torrent up in queue",
      "Clear force start",
    ])
      expect(html).toContain(text);
  });
  it("keeps login labels, autocomplete and expired-session errors accessible", () => {
    const html = renderToStaticMarkup(
      <LoginForm
        initialError="Your session has expired. Sign in again."
        onLogin={async () => {}}
      />,
    );
    expect(html).toContain('autoComplete="username"');
    expect(html).toContain('autoComplete="current-password"');
    expect(html).toContain('role="alert"');
    expect(html).toContain("Your session has expired");
    expect(loginErrorMessage(401)).toBe("Invalid username or password.");
    expect(loginErrorMessage(429)).toBe(
      "Too many attempts. Please try again later.",
    );
    expect(loginErrorMessage(500)).toBe("Unable to connect to rqbit.");
  });
  it("clamps progress and keeps its numeric label readable at zero", () => {
    expect(renderToStaticMarkup(<ProgressBar now={0} />)).toContain("0.0%");
    expect(renderToStaticMarkup(<ProgressBar now={200} />)).toContain(
      'aria-valuenow="100"',
    );
    expect(renderToStaticMarkup(<ProgressBar now={NaN} />)).toContain(
      'aria-valuenow="0"',
    );
  });
});
