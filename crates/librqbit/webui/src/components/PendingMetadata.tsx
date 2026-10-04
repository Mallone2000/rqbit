import { useContext, useState } from "react";
import { APIContext } from "../context";
import { useTorrentStore } from "../stores/torrentStore";
import { useErrorStore } from "../stores/errorStore";
import type { PendingTorrent } from "../api-types";

export const PendingMetadata = ({
  torrents,
}: {
  torrents: PendingTorrent[];
}) => {
  const API = useContext(APIContext);
  const refreshTorrents = useTorrentStore((state) => state.refreshTorrents);
  const setError = useErrorStore((state) => state.setCloseableError);
  const [busy, setBusy] = useState<string | null>(null);
  const act = async (hash: string, action: "pause" | "start" | "forget") => {
    if (!API.pendingAction) return;
    setBusy(hash);
    try {
      await API.pendingAction(hash, action);
      refreshTorrents();
    } catch {
      setError({
        text: "Could not update the pending torrent. Refresh and try again.",
      });
    } finally {
      setBusy(null);
    }
  };
  if (torrents.length === 0) return null;
  return (
    <section
      aria-label="Pending torrent metadata"
      className="shrink-0 max-h-40 overflow-auto border-b border-slate-300 dark:border-slate-700 bg-white dark:bg-slate-800 p-3 text-sm"
    >
      <h2 className="font-semibold">
        Pending torrent metadata ({torrents.length})
      </h2>
      <ul className="space-y-2 mt-2">
        {torrents.map((torrent) => (
          <li key={torrent.info_hash}>
            <span className="font-medium">
              {torrent.name ?? torrent.info_hash}
            </span>
            {torrent.category && <span> ({torrent.category})</span>}
            <p>
              {torrent.state === "paused"
                ? "Paused while waiting for metadata"
                : torrent.state === "error"
                  ? "Could not add torrent"
                  : "Waiting for metadata"}
              .{" "}
              {torrent.state === "paused"
                ? "Metadata discovery is stopped until you resume."
                : (torrent.message ??
                  "Discovering peers and requesting torrent metadata.")}
            </p>
            <p className="text-secondary">
              Attempts: {torrent.metadata_attempts}
              {torrent.next_retry_at_unix_seconds != null && (
                <>
                  {" "}
                  · Next retry:{" "}
                  {new Date(
                    torrent.next_retry_at_unix_seconds * 1000,
                  ).toLocaleTimeString()}
                </>
              )}
            </p>
            {API.pendingAction && (
              <div className="flex gap-3 mt-1">
                <button
                  type="button"
                  disabled={busy !== null}
                  className="underline disabled:opacity-50"
                  onClick={() =>
                    act(
                      torrent.info_hash,
                      torrent.state === "resolving_metadata"
                        ? "pause"
                        : "start",
                    )
                  }
                >
                  {torrent.state === "resolving_metadata"
                    ? "Pause"
                    : torrent.state === "error"
                      ? "Retry"
                      : "Resume"}
                </button>
                <button
                  type="button"
                  disabled={busy !== null}
                  className="underline disabled:opacity-50"
                  onClick={() => act(torrent.info_hash, "forget")}
                >
                  Remove, keep files
                </button>
              </div>
            )}
          </li>
        ))}
      </ul>
    </section>
  );
};
