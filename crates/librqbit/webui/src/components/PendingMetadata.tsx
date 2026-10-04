import type { PendingTorrent } from "../api-types";

export const PendingMetadata = ({
  torrents,
}: {
  torrents: PendingTorrent[];
}) => {
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
              {torrent.state === "error"
                ? "Could not add torrent"
                : "Waiting for metadata"}
              .{" "}
              {torrent.message ??
                "Discovering peers and requesting torrent metadata."}
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
          </li>
        ))}
      </ul>
    </section>
  );
};
