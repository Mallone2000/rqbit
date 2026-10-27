import { useContext, useState } from "react";
import { APIContext } from "../../context";
import { QueueOperation } from "../../api-types";
import { useTorrentStore } from "../../stores/torrentStore";
import { useErrorStore } from "../../stores/errorStore";

export const QueueActions = ({
  hash,
  position,
  forced,
}: {
  hash: string;
  position?: number | null;
  forced?: boolean;
}) => {
  const api = useContext(APIContext);
  const [busy, setBusy] = useState(false);
  const refresh = useTorrentStore((s) => s.refreshTorrents);
  const showError = useErrorStore((s) => s.setCloseableError);
  if (!api.reorderQueue && !api.setForceStart) return null;
  const run = async (operation: () => Promise<void>) => {
    setBusy(true);
    try {
      await operation();
      refresh();
    } catch {
      showError({
        text: "Could not update torrent queue",
        details: { text: "The operation failed. Refresh and try again." },
      });
    } finally {
      setBusy(false);
    }
  };
  return (
    <span
      className="inline-flex flex-wrap gap-1 text-xs"
      onMouseDown={(e) => e.stopPropagation()}
      onClick={(e) => e.stopPropagation()}
    >
      {position != null &&
        api.reorderQueue &&
        (
          [
            ["top", "Top"],
            ["up", "Up"],
            ["down", "Down"],
            ["bottom", "Bottom"],
          ] as [QueueOperation, string][]
        ).map(([operation, label]) => (
          <button
            key={operation}
            type="button"
            aria-label={
              operation === "up" || operation === "down"
                ? `Move torrent ${operation} in queue`
                : `Move torrent to ${operation} of queue`
            }
            disabled={busy}
            className="rounded border border-divider px-1 disabled:opacity-50"
            onClick={() => run(() => api.reorderQueue!([hash], operation))}
          >
            {label}
          </button>
        ))}
      {api.setForceStart && (
        <button
          type="button"
          disabled={busy}
          aria-pressed={!!forced}
          className="rounded border border-divider px-1 disabled:opacity-50"
          onClick={() => run(() => api.setForceStart!(hash, !forced))}
        >
          {forced ? "Clear force start" : "Force start"}
        </button>
      )}
    </span>
  );
};
