import { PendingMetadata } from "./PendingMetadata";
import { CardLayout } from "./CardLayout";
import { ErrorComponent } from "./ErrorComponent";
import { useTorrentStore } from "../stores/torrentStore";
import { useErrorStore } from "../stores/errorStore";
import { useUIStore } from "../stores/uiStore";
import { useIsLargeScreen } from "../hooks/useIsLargeScreen";
import { CompactLayout } from "./compact/CompactLayout";

export const RootContent = (props: {}) => {
  let closeableError = useErrorStore((state) => state.closeableError);
  let setCloseableError = useErrorStore((state) => state.setCloseableError);
  let otherError = useErrorStore((state) => state.otherError);
  let torrents = useTorrentStore((state) => state.torrents);
  let torrentsInitiallyLoading = useTorrentStore(
    (state) => state.torrentsInitiallyLoading,
  );

  const pendingTorrents = useTorrentStore((state) => state.pendingTorrents);
  const viewMode = useUIStore((state) => state.viewMode);
  const isLargeScreen = useIsLargeScreen();

  const useCompactLayout = viewMode === "compact" && isLargeScreen;

  return (
    <div className="h-full flex flex-col">
      <ErrorComponent
        error={closeableError}
        remove={() => setCloseableError(null)}
      />
      <ErrorComponent error={otherError} />
      <PendingMetadata torrents={pendingTorrents} />
      {useCompactLayout ? (
        <div className="flex-1 min-h-0">
          <CompactLayout
            torrents={torrents}
            loading={torrentsInitiallyLoading}
          />
        </div>
      ) : (
        <div className="flex-1 min-h-0">
          <CardLayout torrents={torrents} loading={torrentsInitiallyLoading} />
        </div>
      )}
    </div>
  );
};
