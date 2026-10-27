import { TorrentListItem } from "../api-types";

export function summarizeTorrents(torrents: TorrentListItem[] | null) {
  if (torrents === null || torrents.some((t) => !t.stats)) return null;
  return torrents.reduce(
    (counts, torrent) => {
      const stats = torrent.stats!;
      if (stats.state === "live" && !stats.queued) counts.active++;
      if (stats.queued) counts.queued++;
      if (stats.state === "paused" && !stats.queued) counts.paused++;
      if (stats.finished) counts.completed++;
      return counts;
    },
    { active: 0, queued: 0, paused: 0, completed: 0 },
  );
}
