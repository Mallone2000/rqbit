import { useContext, useState } from "react";
import { TorrentListItem, TorrentStats } from "../../api-types";
import { APIContext } from "../../context";
import { IconButton } from "./IconButton";
import { DeleteTorrentModal } from "../modal/DeleteTorrentModal";
import { FaCog, FaPause, FaPlay, FaTrash } from "react-icons/fa";
import { useErrorStore } from "../../stores/errorStore";
import { useTorrentStore } from "../../stores/torrentStore";
import { useUIStore } from "../../stores/uiStore";
import { QueueActions } from "./QueueActions";
import { PlaylistIconButton } from "./PlaylistButton";

export const TorrentActions: React.FC<{
  torrent: TorrentListItem & { stats: TorrentStats };
}> = ({ torrent }) => {
  const { id, name, stats } = torrent;
  const state = stats.state;

  let [disabled, setDisabled] = useState<boolean>(false);
  let [deleting, setDeleting] = useState<boolean>(false);

  const refreshTorrents = useTorrentStore((state) => state.refreshTorrents);
  const openDetailsModal = useUIStore((state) => state.openDetailsModal);

  const canPause = state == "live" || !!stats.queued;
  const canUnpause = (state == "paused" && !stats.queued) || state == "error";
  const canConfigure = state == "paused" || state == "live";

  const setCloseableError = useErrorStore((state) => state.setCloseableError);

  const API = useContext(APIContext);

  const unpause = () => {
    setDisabled(true);
    API.start(id)
      .then(
        () => {
          refreshTorrents();
        },
        (e) => {
          setCloseableError({
            text: `Error starting torrent id=${id}`,
            details: e,
          });
        },
      )
      .finally(() => setDisabled(false));
  };

  const pause = () => {
    setDisabled(true);
    API.pause(id)
      .then(
        () => {
          refreshTorrents();
        },
        (e) => {
          setCloseableError({
            text: `Error pausing torrent id=${id}`,
            details: e,
          });
        },
      )
      .finally(() => setDisabled(false));
  };

  const startDeleting = () => {
    setDisabled(true);
    setDeleting(true);
  };

  const cancelDeleting = () => {
    setDisabled(false);
    setDeleting(false);
  };

  return (
    <div className="flex flex-wrap w-full justify-center gap-1 sm:gap-2 dark:text-slate-300">
      {canUnpause && (
        <IconButton
          onClick={unpause}
          title="Resume torrent"
          disabled={disabled}
        >
          <FaPlay className="hover:text-green-600" />
        </IconButton>
      )}
      {canPause && (
        <IconButton onClick={pause} title="Pause torrent" disabled={disabled}>
          <FaPause className="hover:text-amber-500" />
        </IconButton>
      )}
      {canConfigure && (
        <IconButton
          onClick={() => openDetailsModal(id)}
          title="Torrent details"
          disabled={disabled}
        >
          <FaCog className="hover:text-green-600" />
        </IconButton>
      )}
      <IconButton
        onClick={startDeleting}
        title="Delete torrent"
        disabled={disabled}
      >
        <FaTrash className="hover:text-red-500" />
      </IconButton>
      <QueueActions
        hash={torrent.info_hash}
        position={stats.queue_position}
        forced={stats.force_start}
      />
      <PlaylistIconButton torrentId={id} disabled={disabled} />
      <DeleteTorrentModal
        show={deleting}
        onHide={cancelDeleting}
        torrents={[{ id, name }]}
      />
    </div>
  );
};
