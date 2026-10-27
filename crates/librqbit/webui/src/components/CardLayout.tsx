import { useState, useCallback, useMemo } from "react";
import { Virtuoso } from "react-virtuoso";
import { TorrentListItem } from "../api-types";
import { Spinner } from "./Spinner";
import { TorrentCard } from "./TorrentCard";
import { TorrentDetailsModal } from "./modal/TorrentDetailsModal";
import { useUIStore } from "../stores/uiStore";
import {
  TorrentSortColumn,
  SortDirection,
  SORT_COLUMN_LABELS,
  compareTorrents,
  isTorrentVisible,
} from "../helper/torrentFilters";
import { useKeyboardShortcuts } from "../hooks/useKeyboardShortcuts";

const DEFAULT_SORT_COLUMN: TorrentSortColumn = "id";
const DEFAULT_SORT_DIRECTION: SortDirection = "desc";

export const CardLayout = (props: {
  torrents: Array<TorrentListItem> | null;
  loading: boolean;
}) => {
  const searchQuery = useUIStore((state) => state.searchQuery);
  const statusFilter = useUIStore((state) => state.statusFilter);

  // Keyboard shortcuts (Ctrl+A, Ctrl+F, Escape)
  useKeyboardShortcuts();
  const detailsModalTorrentId = useUIStore(
    (state) => state.detailsModalTorrentId,
  );
  const closeDetailsModal = useUIStore((state) => state.closeDetailsModal);

  const [sortColumn, setSortColumn] =
    useState<TorrentSortColumn>(DEFAULT_SORT_COLUMN);
  const [sortDirection, setSortDirection] = useState<SortDirection>(
    DEFAULT_SORT_DIRECTION,
  );

  const handleSortChange = (e: React.ChangeEvent<HTMLSelectElement>) => {
    const [col, dir] = e.target.value.split(":") as [
      TorrentSortColumn,
      SortDirection,
    ];
    setSortColumn(col);
    setSortDirection(dir);
  };

  const normalizedQuery = searchQuery.toLowerCase().trim();

  // Sort and filter torrents for virtualization
  const filteredTorrents = useMemo(() => {
    if (!props.torrents) return null;
    return [...props.torrents]
      .filter((t) => isTorrentVisible(t, normalizedQuery, statusFilter))
      .sort((a, b) => compareTorrents(a, b, sortColumn, sortDirection));
  }, [
    props.torrents,
    normalizedQuery,
    statusFilter,
    sortColumn,
    sortDirection,
  ]);

  // Item renderer for react-virtuoso
  const itemContent = useCallback(
    (index: number) => {
      const torrent = filteredTorrents![index];
      return (
        <div className="pb-4 max-w-6xl mx-auto w-full">
          <TorrentCard key={torrent.id} torrent={torrent} />
        </div>
      );
    },
    [filteredTorrents],
  );

  return (
    <div className="flex flex-col h-full">
      <div className="flex shrink-0 justify-end mb-3">
        {/* Sort dropdown */}
        <select
          aria-label="Sort torrents"
          value={`${sortColumn}:${sortDirection}`}
          onChange={handleSortChange}
          className="py-1.5 sm:py-2 px-2 sm:px-3 text-sm bg-surface border border-divider rounded-lg focus:outline-none focus:border-primary"
        >
          {(Object.keys(SORT_COLUMN_LABELS) as TorrentSortColumn[]).flatMap(
            (col) => [
              <option key={`${col}:desc`} value={`${col}:desc`}>
                {SORT_COLUMN_LABELS[col]} ↓
              </option>,
              <option key={`${col}:asc`} value={`${col}:asc`}>
                {SORT_COLUMN_LABELS[col]} ↑
              </option>,
            ],
          )}
        </select>
      </div>
      {filteredTorrents === null ? (
        props.loading ? (
          <Spinner
            className="justify-center m-5"
            label="Loading torrent list"
          />
        ) : null
      ) : filteredTorrents.length === 0 ? (
        <div className="p-8 text-center text-secondary">
          <p className="text-xl font-bold text-text">
            {props.torrents?.length
              ? "No matching torrents"
              : "Your torrent list is empty"}
          </p>
          <p className="mt-2">
            {props.torrents?.length
              ? "Try another search or status filter."
              : "Add a magnet link or torrent file to get started."}
          </p>
        </div>
      ) : (
        <div className="flex-1 min-h-0">
          <Virtuoso
            totalCount={filteredTorrents.length}
            itemContent={itemContent}
          />
        </div>
      )}
      {detailsModalTorrentId !== null && (
        <TorrentDetailsModal
          torrentId={detailsModalTorrentId}
          isOpen={true}
          onClose={closeDetailsModal}
        />
      )}
    </div>
  );
};
