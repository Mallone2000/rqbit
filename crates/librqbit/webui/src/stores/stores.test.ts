import { beforeEach, describe, expect, it, vi } from "vitest";
import type { TorrentDetails, TorrentListItem } from "../api-types";
import { useErrorStore } from "./errorStore";
import { useStatsStore } from "./statsStore";
import { useTorrentStore } from "./torrentStore";

vi.stubGlobal("window", { innerWidth: 1200 });
const { useUIStore } = await import("./uiStore");

function torrent(id: number, name: string): TorrentListItem {
  return {
    id,
    name,
    info_hash: String(id),
    output_folder: "",
    total_pieces: 1,
  };
}

describe("torrent store", () => {
  beforeEach(() => {
    useTorrentStore.setState({
      torrents: null,
      torrentsLoading: false,
      torrentsInitiallyLoading: false,
      detailsCache: new Map(),
    });
  });

  it("tracks initial loading separately from refresh loading", () => {
    useTorrentStore.getState().setTorrentsLoading(true);
    expect(useTorrentStore.getState().torrentsInitiallyLoading).toBe(true);
    useTorrentStore.getState().setTorrents([torrent(1, "one")]);
    useTorrentStore.getState().setTorrentsLoading(false);
    expect(useTorrentStore.getState().torrentsInitiallyLoading).toBe(false);
  });

  it("preserves references for unchanged torrents and replaces changed ones", () => {
    const original = torrent(1, "one");
    useTorrentStore.getState().setTorrents([original]);
    useTorrentStore.getState().setTorrents([torrent(1, "one")]);
    expect(useTorrentStore.getState().torrents?.[0]).toBe(original);

    useTorrentStore.getState().setTorrents([torrent(1, "changed")]);
    expect(useTorrentStore.getState().torrents?.[0]).not.toBe(original);
  });

  it("stores callbacks and immutable details cache entries", () => {
    const refresh = vi.fn();
    useTorrentStore.getState().setRefreshTorrents(refresh);
    useTorrentStore.getState().refreshTorrents();
    expect(refresh).toHaveBeenCalledOnce();

    const oldCache = useTorrentStore.getState().detailsCache;
    const details = { name: "one" } as TorrentDetails;
    useTorrentStore.getState().setDetails(1, details);
    expect(useTorrentStore.getState().getDetails(1)).toBe(details);
    expect(useTorrentStore.getState().getDetails(2)).toBeNull();
    expect(useTorrentStore.getState().detailsCache).not.toBe(oldCache);
  });
});

describe("UI store", () => {
  beforeEach(() => {
    useUIStore.setState({
      viewMode: "compact",
      searchQuery: "",
      statusFilter: "all",
      selectedTorrentIds: new Set(),
      lastSelectedId: null,
      detailsModalTorrentId: null,
    });
  });

  it("updates view, query, and filter state", () => {
    useUIStore.getState().toggleViewMode();
    useUIStore.getState().setSearchQuery("linux");
    useUIStore.getState().setStatusFilter("seeding");
    expect(useUIStore.getState()).toMatchObject({
      viewMode: "full",
      searchQuery: "linux",
      statusFilter: "seeding",
    });
  });

  it("supports single, toggle, range, all, deselect, and clear selection", () => {
    const store = () => useUIStore.getState();
    store().selectTorrent(2);
    store().selectRange(4, [1, 2, 3, 4]);
    expect([...store().selectedTorrentIds]).toEqual([2, 3, 4]);
    store().toggleSelection(3);
    expect(store().selectedTorrentIds.has(3)).toBe(false);
    store().deselectTorrent(2);
    expect([...store().selectedTorrentIds]).toEqual([4]);
    store().selectAll([1, 2]);
    expect([...store().selectedTorrentIds]).toEqual([1, 2]);
    store().clearSelection();
    expect(store().selectedTorrentIds.size).toBe(0);
    expect(store().lastSelectedId).toBeNull();
  });

  it("moves relative selection with clamping and empty-selection defaults", () => {
    const store = () => useUIStore.getState();
    store().selectRelative("down", [1, 2, 3]);
    expect([...store().selectedTorrentIds]).toEqual([1]);
    store().selectRelative("down", [1, 2, 3]);
    expect([...store().selectedTorrentIds]).toEqual([2]);
    store().selectRelative("up", [1, 2, 3]);
    expect([...store().selectedTorrentIds]).toEqual([1]);
    store().clearSelection();
    store().selectRelative("up", [1, 2, 3]);
    expect([...store().selectedTorrentIds]).toEqual([3]);
  });

  it("opening details also selects the torrent", () => {
    useUIStore.getState().openDetailsModal(9);
    expect(useUIStore.getState().detailsModalTorrentId).toBe(9);
    expect([...useUIStore.getState().selectedTorrentIds]).toEqual([9]);
    useUIStore.getState().closeDetailsModal();
    expect(useUIStore.getState().detailsModalTorrentId).toBeNull();
  });
});

describe("simple stores", () => {
  it("sets and clears each error channel", () => {
    const error = { text: "boom" };
    useErrorStore.getState().setAlert(error);
    useErrorStore.getState().setCloseableError(error);
    useErrorStore.getState().setOtherError(error);
    expect(useErrorStore.getState()).toMatchObject({
      alert: error,
      closeableError: error,
      otherError: error,
    });
    useErrorStore.getState().removeAlert();
    useErrorStore.getState().setCloseableError(null);
    useErrorStore.getState().setOtherError(null);
    expect(useErrorStore.getState()).toMatchObject({
      alert: null,
      closeableError: null,
      otherError: null,
    });
  });

  it("replaces session stats", () => {
    const stats = {
      ...useStatsStore.getState().stats,
      uptime_seconds: 42,
    };
    useStatsStore.getState().setStats(stats);
    expect(useStatsStore.getState().stats).toBe(stats);
  });
});
