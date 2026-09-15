import { afterEach, describe, expect, it, vi } from "vitest";
import type { TorrentListItem, TorrentStats } from "../api-types";
import { customSetInterval } from "./customSetInterval";
import { formatBytes } from "./formatBytes";
import { formatSecondsToTime } from "./formatSecondsToTime";
import { getCompletionETA } from "./getCompletionETA";
import { loopUntilSuccess } from "./loopUntilSuccess";
import {
  compareTorrents,
  getSortValue,
  isTorrentVisible,
  matchesSearch,
  matchesStatus,
} from "./torrentFilters";

function torrent(
  id: number,
  name: string | null,
  state: TorrentStats["state"] = "live",
  finished = false,
): TorrentListItem {
  return {
    id,
    name,
    info_hash: String(id),
    output_folder: "",
    total_pieces: 1,
    stats: {
      state,
      finished,
      error: null,
      file_progress: [],
      progress_bytes: 25,
      total_bytes: 100,
      live:
        state === "live"
          ? ({
              download_speed: { mbps: 2, human_readable: "2 MiB/s" },
              upload_speed: { mbps: 1, human_readable: "1 MiB/s" },
            } as TorrentStats["live"])
          : null,
    },
  };
}

describe("formatters", () => {
  it.each([
    [0, "0 Bytes"],
    [1, "1 Bytes"],
    [1024, "1 KB"],
    [1536, "1.5 KB"],
    [1024 ** 3, "1 GB"],
  ])("formats %d bytes", (value, expected) => {
    expect(formatBytes(value)).toBe(expected);
  });

  it.each([
    [0, ""],
    [1, "1s"],
    [61, "1m 1s"],
    [3600, "1h"],
    [90060, "1d 1h 1m"],
  ])("formats %d seconds", (value, expected) => {
    expect(formatSecondsToTime(value)).toBe(expected);
  });

  it("formats completion ETA and handles missing estimates", () => {
    expect(getCompletionETA({ live: null } as TorrentStats)).toBe("N/A");
    expect(
      getCompletionETA({
        live: { time_remaining: { duration: { secs: 61 } } },
      } as TorrentStats),
    ).toBe("1m 1s");
  });
});

describe("torrent filtering and sorting", () => {
  const alpha = torrent(2, "Alpha");
  const beta = torrent(1, "beta", "live", true);
  const paused = torrent(3, null, "paused");

  it("extracts every sortable value and computes ETA", () => {
    expect(getSortValue(alpha, "id")).toBe(2);
    expect(getSortValue(alpha, "name")).toBe("alpha");
    expect(getSortValue(alpha, "size")).toBe(100);
    expect(getSortValue(alpha, "progress")).toBe(0.25);
    expect(getSortValue(alpha, "downSpeed")).toBe(2);
    expect(getSortValue(alpha, "upSpeed")).toBe(1);
    expect(getSortValue(alpha, "eta")).toBe(75 / (2 * 1024 * 1024));
    expect(getSortValue(paused, "eta")).toBe(Infinity);
  });

  it("sorts strings and numbers in either direction", () => {
    expect(compareTorrents(alpha, beta, "name", "asc")).toBeLessThan(0);
    expect(compareTorrents(alpha, beta, "id", "desc")).toBeLessThan(0);
  });

  it("matches search and each status", () => {
    expect(matchesSearch("Alpha", "alp")).toBe(true);
    expect(matchesSearch(null, "x")).toBe(false);
    expect(matchesStatus(alpha, "downloading")).toBe(true);
    expect(matchesStatus(beta, "seeding")).toBe(true);
    expect(matchesStatus(paused, "paused")).toBe(true);
    expect(matchesStatus(torrent(4, "bad", "error"), "error")).toBe(true);
    expect(matchesStatus(alpha, "all")).toBe(true);
    expect(isTorrentVisible(alpha, "alp", "downloading")).toBe(true);
    expect(isTorrentVisible(alpha, "missing", "downloading")).toBe(false);
  });
});

describe("timer helpers", () => {
  afterEach(() => vi.useRealTimers());

  it("retries until callback succeeds and cancellation stops future retries", async () => {
    vi.useFakeTimers();
    const callback = vi
      .fn<() => Promise<void>>()
      .mockRejectedValueOnce(new Error("retry"))
      .mockResolvedValue(undefined);
    const cancel = loopUntilSuccess(callback, 100);

    await vi.advanceTimersByTimeAsync(0);
    expect(callback).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(100);
    expect(callback).toHaveBeenCalledTimes(2);
    cancel();
    await vi.advanceTimersByTimeAsync(1000);
    expect(callback).toHaveBeenCalledTimes(2);
  });

  it("uses each interval returned by an async callback", async () => {
    vi.useFakeTimers();
    const callback = vi
      .fn<() => Promise<number>>()
      .mockResolvedValueOnce(20)
      .mockResolvedValue(30);
    const cancel = customSetInterval(callback, 10);

    await vi.advanceTimersByTimeAsync(10);
    expect(callback).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(19);
    expect(callback).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(callback).toHaveBeenCalledTimes(2);
    cancel();
  });
});
