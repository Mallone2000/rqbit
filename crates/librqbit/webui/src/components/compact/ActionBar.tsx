import { useContext, useState, useCallback, useMemo } from "react";
import { FaPause, FaPlay, FaTrash } from "react-icons/fa";
import { APIContext } from "../../context";
import { useUIStore } from "../../stores/uiStore";
import { useTorrentStore } from "../../stores/torrentStore";
import { useErrorStore } from "../../stores/errorStore";
import { DeleteTorrentModal } from "../modal/DeleteTorrentModal";
import { useKeyboardShortcuts } from "../../hooks/useKeyboardShortcuts";
import {
  ErrorDetails,
  STATE_LIVE,
  STATE_PAUSED,
  TorrentListItem,
} from "../../api-types";
import { Button } from "../buttons/Button";

interface ActionBarProps {
  // When true, hides search/filter/selection count (for use in modal)
  hideFilters?: boolean;
  children?: React.ReactNode;
}

export const ActionBar: React.FC<ActionBarProps> = ({
  hideFilters,
  children,
}) => {
  const selectedTorrentIds = useUIStore((state) => state.selectedTorrentIds);
  const torrents = useTorrentStore((state) => state.torrents);
  const refreshTorrents = useTorrentStore((state) => state.refreshTorrents);
  const setCloseableError = useErrorStore((state) => state.setCloseableError);

  const [disabled, setDisabled] = useState(false);
  const [showDeleteModal, setShowDeleteModal] = useState(false);
  const [torrentsToDelete, setTorrentsToDelete] = useState<
    Pick<TorrentListItem, "id" | "name">[]
  >([]);
  const API = useContext(APIContext);

  const selectedCount = selectedTorrentIds.size;
  const hasSelection = selectedCount > 0;

  const getTorrentById = (id: number) => torrents?.find((t) => t.id === id);

  const openDeleteModal = useCallback(() => {
    // Capture current selection when opening modal (stable snapshot)
    const torrentsList = Array.from(selectedTorrentIds).map((id) => {
      const torrent = getTorrentById(id);
      return {
        id,
        name: torrent?.name ?? null,
      };
    });
    setTorrentsToDelete(torrentsList);
    setShowDeleteModal(true);
  }, [selectedTorrentIds, torrents]);

  // Keyboard shortcuts for compact view
  const keyboardActions = useMemo(
    () => ({ onDelete: openDeleteModal }),
    [openDeleteModal],
  );
  useKeyboardShortcuts(keyboardActions);

  const runBulkAction = async (
    action: (id: number) => Promise<void>,
    skipState: string,
    errorLabel: string,
  ) => {
    setDisabled(true);
    try {
      for (const id of selectedTorrentIds) {
        const torrent = getTorrentById(id);
        if (torrent?.stats?.state === skipState) continue;
        try {
          await action(id);
          refreshTorrents();
        } catch (e) {
          setCloseableError({
            text: `Error ${errorLabel} torrent id=${id}`,
            details: e as ErrorDetails,
          });
        }
      }
    } finally {
      setDisabled(false);
    }
  };

  const pauseSelected = () =>
    runBulkAction((id) => API.pause(id), STATE_PAUSED, "pausing");
  const resumeSelected = () =>
    runBulkAction((id) => API.start(id), STATE_LIVE, "starting");

  return (
    <div className="flex shrink-0 flex-wrap items-center gap-2 px-4 py-3 bg-surface-raised border-b border-divider">
      <Button
        onClick={resumeSelected}
        disabled={disabled || !hasSelection}
        variant="secondary"
      >
        <FaPlay className="w-2.5 h-2.5" />
        Resume
      </Button>
      <Button
        onClick={pauseSelected}
        disabled={disabled || !hasSelection}
        variant="secondary"
      >
        <FaPause className="w-2.5 h-2.5" />
        Pause
      </Button>
      <Button
        onClick={openDeleteModal}
        disabled={disabled || !hasSelection}
        variant="danger"
      >
        <FaTrash className="w-2.5 h-2.5" />
        Delete
      </Button>

      {!hideFilters && (
        <>
          {hasSelection && (
            <span className="ml-1.5 text-secondary">
              {selectedCount} selected
            </span>
          )}
        </>
      )}

      {children && <div className="ml-auto">{children}</div>}
      <DeleteTorrentModal
        show={showDeleteModal}
        onHide={() => setShowDeleteModal(false)}
        torrents={torrentsToDelete}
      />
    </div>
  );
};
