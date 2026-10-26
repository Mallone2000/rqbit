import { useContext, useEffect, useState } from "react";
import { APIContext } from "../context";
import { useTorrentStore } from "../stores/torrentStore";
import { useErrorStore } from "../stores/errorStore";
import type { PendingTorrent, RqbitAPI } from "../api-types";
import { Button } from "./buttons/Button";

type PendingAction = "pause" | "start" | "forget";

function pendingActionErrorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (
    error !== null &&
    typeof error === "object" &&
    "text" in error &&
    typeof error.text === "string"
  ) {
    return error.text;
  }
  return "Request failed";
}

export async function executePendingActions(
  torrents: PendingTorrent[],
  action: PendingAction,
  pendingAction: NonNullable<RqbitAPI["pendingAction"]>,
) {
  const succeeded: string[] = [];
  const failed: string[] = [];
  for (const torrent of torrents) {
    try {
      await pendingAction(torrent.info_hash, action);
      succeeded.push(torrent.info_hash);
    } catch (error) {
      failed.push(
        `${torrent.name ?? torrent.info_hash}: ${pendingActionErrorMessage(error)}`,
      );
    }
  }
  return { succeeded, failed };
}

export const PendingMetadata = ({
  torrents,
}: {
  torrents: PendingTorrent[];
}) => {
  const API = useContext(APIContext);
  const refreshTorrents = useTorrentStore((state) => state.refreshTorrents);
  const setError = useErrorStore((state) => state.setCloseableError);
  const [busy, setBusy] = useState(false);
  const [selectedHashes, setSelectedHashes] = useState<Set<string>>(new Set());
  const selectedTorrents = torrents.filter((torrent) =>
    selectedHashes.has(torrent.info_hash),
  );
  const allSelected = selectedTorrents.length === torrents.length;
  const pauseTargets = selectedTorrents.filter(
    (torrent) => torrent.state === "resolving_metadata",
  );
  const startTargets = selectedTorrents.filter(
    (torrent) => torrent.state !== "resolving_metadata",
  );

  useEffect(() => {
    const currentHashes = new Set(torrents.map((torrent) => torrent.info_hash));
    setSelectedHashes((previous) => {
      const retained = [...previous].filter((hash) => currentHashes.has(hash));
      return retained.length === previous.size ? previous : new Set(retained);
    });
  }, [torrents]);

  const toggleSelection = (hash: string) => {
    setSelectedHashes((previous) => {
      const next = new Set(previous);
      if (next.has(hash)) next.delete(hash);
      else next.add(hash);
      return next;
    });
  };

  const act = async (targets: PendingTorrent[], action: PendingAction) => {
    if (!API.pendingAction) return;
    setBusy(true);
    try {
      const { succeeded, failed } = await executePendingActions(
        targets,
        action,
        API.pendingAction,
      );
      if (action === "forget") {
        setSelectedHashes((previous) => {
          const next = new Set(previous);
          succeeded.forEach((hash) => next.delete(hash));
          return next;
        });
      }
      if (failed.length > 0) {
        setError({
          text: `Could not update ${failed.length} pending torrent${failed.length === 1 ? "" : "s"}.`,
          details: { text: failed.join("\n") },
        });
      }
      refreshTorrents();
    } finally {
      setBusy(false);
    }
  };
  if (torrents.length === 0) return null;
  return (
    <section
      aria-label="Pending torrent metadata"
      aria-busy={busy}
      className="shrink-0 max-h-64 overflow-auto border-b border-slate-300 dark:border-slate-700 bg-white dark:bg-slate-800 p-3 text-sm"
    >
      <h2 className="font-semibold">
        Pending torrent metadata ({torrents.length})
      </h2>
      {API.pendingAction && (
        <div className="mt-2 flex flex-wrap items-center gap-2">
          <label className="inline-flex items-center gap-2 mr-2">
            <input
              type="checkbox"
              checked={allSelected}
              ref={(input) => {
                if (input)
                  input.indeterminate =
                    selectedTorrents.length > 0 && !allSelected;
              }}
              disabled={busy}
              onChange={(event) =>
                setSelectedHashes(
                  event.target.checked
                    ? new Set(torrents.map((torrent) => torrent.info_hash))
                    : new Set(),
                )
              }
            />
            Select all
          </label>
          <Button
            size="sm"
            disabled={busy || pauseTargets.length === 0}
            className="disabled:opacity-50"
            onClick={() => act(pauseTargets, "pause")}
          >
            Pause selected ({pauseTargets.length})
          </Button>
          <Button
            size="sm"
            disabled={busy || startTargets.length === 0}
            className="disabled:opacity-50"
            onClick={() => act(startTargets, "start")}
          >
            Resume / retry selected ({startTargets.length})
          </Button>
          <Button
            size="sm"
            variant="danger"
            disabled={busy || selectedTorrents.length === 0}
            className="disabled:opacity-50"
            onClick={() => act(selectedTorrents, "forget")}
          >
            Remove selected, keep files ({selectedTorrents.length})
          </Button>
          {busy && <span role="status">Applying actions…</span>}
        </div>
      )}
      <ul className="space-y-2 mt-2">
        {torrents.map((torrent) => (
          <li key={torrent.info_hash}>
            {API.pendingAction && (
              <input
                type="checkbox"
                aria-label={`Select ${torrent.name ?? torrent.info_hash}`}
                checked={selectedHashes.has(torrent.info_hash)}
                disabled={busy}
                onChange={() => toggleSelection(torrent.info_hash)}
                className="mr-2"
              />
            )}
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
                  disabled={busy}
                  className="underline disabled:opacity-50"
                  onClick={() =>
                    act(
                      [torrent],
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
                  disabled={busy}
                  className="underline disabled:opacity-50"
                  onClick={() => act([torrent], "forget")}
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
