import { TorrentListItem, STATE_INITIALIZING } from "../../api-types";

import { formatBytes } from "../../helper/formatBytes";
import { getCompletionETA } from "../../helper/getCompletionETA";
import { torrentStatusLabel } from "../../helper/torrentStatusLabel";
import { memo } from "react";

interface TorrentTableRowProps {
  torrent: TorrentListItem;
  isSelected: boolean;
  onRowClick: (id: number, e: React.MouseEvent) => void;
  onCheckboxChange: (id: number) => void;
}

const TorrentTableRowUnmemoized: React.FC<TorrentTableRowProps> = ({
  torrent,
  isSelected,
  onRowClick,
  onCheckboxChange,
}) => {
  const stats = torrent.stats;
  const state = stats?.state ?? "";
  const error = stats?.error ?? null;
  const totalBytes = stats?.total_bytes ?? 1;
  const progressBytes = stats?.progress_bytes ?? 0;
  const finished = stats?.finished || false;

  const progressPercentage = error
    ? 100
    : totalBytes === 0
      ? 100
      : Math.round((progressBytes / totalBytes) * 100);

  const downloadSpeed = stats?.live?.download_speed?.human_readable ?? "-";
  const uploadSpeed = stats?.live?.upload_speed?.human_readable ?? "-";

  const peerStats = stats?.live?.snapshot.peer_stats;
  const peersDisplay = peerStats ? `${peerStats.live}/${peerStats.seen}` : "-";

  const eta = stats ? getCompletionETA(stats) : "-";
  const displayEta = finished ? "Done" : eta;

  const name = torrent.name ?? "";

  const handleRowClick = (e: React.MouseEvent) => {
    onRowClick(torrent.id, e);
  };

  // Common cell styles to avoid repetition
  const cellBase = "px-2 align-middle";
  const numericCell = `w-28 ${cellBase} text-right text-secondary whitespace-nowrap`;
  const centeredCell = `w-28 ${cellBase} text-center text-secondary whitespace-nowrap`;

  // Use table-fixed layout to match header column widths
  return (
    <table className="w-full table-fixed">
      <tbody>
        <tr
          onClick={handleRowClick}
          className={`cursor-pointer border-b border-divider text-base h-16 tabular-nums ${
            isSelected ? "bg-primary/10" : "hover:bg-surface-raised"
          }`}
        >
          <td
            className="w-14 px-1 text-center align-middle"
            onClick={(e) => e.stopPropagation()}
          >
            <label className="inline-flex min-h-11 min-w-11 items-center justify-center">
              <input
                type="checkbox"
                checked={isSelected}
                aria-label={`Select ${name || `torrent ${torrent.id}`}`}
                onChange={() => onCheckboxChange(torrent.id)}
                className="w-4 h-4 rounded border-divider-strong bg-surface text-primary focus:ring-primary"
              />
            </label>
          </td>
          <td className={`${cellBase} font-semibold`}>
            <div className="truncate" title={name}>
              <button
                type="button"
                className="block w-full text-left truncate"
                aria-label={`View details for ${name || torrent.id}`}
                onClick={(e) => {
                  e.stopPropagation();
                  handleRowClick(e);
                }}
              >
                {name || "Loading..."}
              </button>
            </div>
            <div className="text-sm text-secondary">
              {torrentStatusLabel(stats)}
            </div>
            {error && (
              <div className="truncate text-sm text-error" title={error}>
                {error}
              </div>
            )}
          </td>
          <td className={numericCell}>{formatBytes(totalBytes)}</td>
          <td className={`w-36 ${cellBase} text-center`}>
            <div className="flex items-center gap-2">
              <div className="flex-1 h-1.5 bg-divider rounded-full overflow-hidden">
                <div
                  className={`h-full rounded-full ${
                    error
                      ? "bg-error-bg"
                      : finished
                        ? "bg-success-bg"
                        : state === STATE_INITIALIZING
                          ? "bg-warning-bg"
                          : "bg-primary-bg"
                  }`}
                  style={{ width: `${progressPercentage}%` }}
                />
              </div>
              <span className="text-sm text-secondary w-8 text-right">
                {progressPercentage}%
              </span>
            </div>
          </td>
          <td className={numericCell}>{downloadSpeed}</td>
          <td className={numericCell}>{uploadSpeed}</td>
          <td className={centeredCell}>{displayEta}</td>
          <td
            className={`w-16 ${cellBase} text-center text-secondary whitespace-nowrap`}
          >
            {peersDisplay}
          </td>
        </tr>
      </tbody>
    </table>
  );
};

export const TorrentTableRow = memo(TorrentTableRowUnmemoized);
