import { TorrentStats } from "../api-types";
export function torrentStatusLabel(stats?: TorrentStats | null): string {
  if (!stats) return "Awaiting stats";
  if (stats.queued)
    return stats.finished ? "Queued for seeding" : "Queued for download";
  if (stats.state === "error") return "Error";
  if (stats.state === "initializing") return "Checking files";
  if (stats.state === "paused") return "Paused";
  return stats.finished ? "Seeding" : "Downloading";
}
