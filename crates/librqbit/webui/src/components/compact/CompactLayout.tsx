import { useState, useCallback, useEffect, useRef } from "react";
import { TorrentListItem } from "../../api-types";
import { ActionBar } from "./ActionBar";
import {
  TorrentTable,
  TableSortColumn,
  TABLE_SORT_LABELS,
} from "./TorrentTable";
import { SortDirection } from "../../helper/torrentFilters";
import { useUIStore } from "../../stores/uiStore";
import { DetailPane } from "./DetailPane";

const DETAIL_PANE_MIN_HEIGHT = 100;
const DETAIL_PANE_MAX_HEIGHT = 600;
const DETAIL_PANE_DEFAULT_HEIGHT = 256;

interface CompactLayoutProps {
  torrents: TorrentListItem[] | null;
  loading: boolean;
}

export const CompactLayout: React.FC<CompactLayoutProps> = ({
  torrents,
  loading,
}) => {
  const [detailPaneHeight, setDetailPaneHeight] = useState(
    DETAIL_PANE_DEFAULT_HEIGHT,
  );
  const [sort, setSort] = useState<{
    column: TableSortColumn;
    direction: SortDirection;
  }>({ column: "id", direction: "desc" });
  const hasSelection = useUIStore((s) => s.selectedTorrentIds.size > 0);
  const [collapsed, setCollapsed] = useState(false);
  const [isDragging, setIsDragging] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);

  const handleMouseDown = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    setIsDragging(true);
  }, []);

  useEffect(() => {
    if (!isDragging) return;

    const handleMouseMove = (e: MouseEvent) => {
      if (!containerRef.current) return;

      const containerRect = containerRef.current.getBoundingClientRect();
      const newHeight = containerRect.bottom - e.clientY;
      const clampedHeight = Math.max(
        DETAIL_PANE_MIN_HEIGHT,
        Math.min(DETAIL_PANE_MAX_HEIGHT, newHeight),
      );
      setDetailPaneHeight(clampedHeight);
    };

    const handleMouseUp = () => {
      setIsDragging(false);
    };

    document.addEventListener("mousemove", handleMouseMove);
    document.addEventListener("mouseup", handleMouseUp);

    return () => {
      document.removeEventListener("mousemove", handleMouseMove);
      document.removeEventListener("mouseup", handleMouseUp);
    };
  }, [isDragging]);

  const hasTorrents = (torrents?.length ?? 0) > 0;

  return (
    <div
      ref={containerRef}
      className="flex flex-col h-full min-h-0 rounded-2xl border border-divider overflow-hidden bg-surface"
    >
      {hasTorrents && (
        <ActionBar>
          <select
            aria-label="Sort torrents"
            value={`${sort.column}:${sort.direction}`}
            onChange={(e) => {
              const [column, direction] = e.target.value.split(":") as [
                TableSortColumn,
                SortDirection,
              ];
              setSort({ column, direction });
            }}
            className="border border-divider bg-surface px-3 py-2"
          >
            {(Object.keys(TABLE_SORT_LABELS) as TableSortColumn[]).flatMap(
              (column) =>
                (["desc", "asc"] as const).map((direction) => (
                  <option
                    key={`${column}:${direction}`}
                    value={`${column}:${direction}`}
                  >
                    {TABLE_SORT_LABELS[column]}{" "}
                    {direction === "asc" ? "↑" : "↓"}
                  </option>
                )),
            )}
          </select>
        </ActionBar>
      )}
      <div className="flex-1 overflow-auto min-h-0">
        <TorrentTable
          torrents={torrents}
          loading={loading}
          sortColumn={sort.column}
          sortDirection={sort.direction}
          onSort={(column) =>
            setSort((prev) => ({
              column,
              direction:
                prev.column === column && prev.direction === "desc"
                  ? "asc"
                  : "desc",
            }))
          }
        />
      </div>
      {hasTorrents && (
        <>
          <div className="flex shrink-0 justify-between items-center border-t border-divider bg-surface-raised px-4">
            <span className="font-semibold">Torrent details</span>
            {hasSelection ? (
              <button
                type="button"
                aria-expanded={!collapsed}
                onClick={() => setCollapsed(!collapsed)}
                className="px-4 text-primary font-semibold"
              >
                {collapsed ? "Show details" : "Hide details"}
              </button>
            ) : (
              <span className="py-3 text-sm text-secondary">
                Select a torrent to view details
              </span>
            )}
          </div>
          {!collapsed && hasSelection && (
            <>
              {/* Draggable divider */}
              <div
                role="separator"
                tabIndex={0}
                aria-label="Resize torrent details"
                aria-orientation="horizontal"
                aria-valuemin={DETAIL_PANE_MIN_HEIGHT}
                aria-valuemax={DETAIL_PANE_MAX_HEIGHT}
                aria-valuenow={detailPaneHeight}
                onKeyDown={(e) => {
                  if (!["ArrowUp", "ArrowDown", "Home", "End"].includes(e.key))
                    return;
                  e.preventDefault();
                  e.stopPropagation();
                  setDetailPaneHeight((height) =>
                    e.key === "Home"
                      ? DETAIL_PANE_MIN_HEIGHT
                      : e.key === "End"
                        ? DETAIL_PANE_MAX_HEIGHT
                        : Math.max(
                            DETAIL_PANE_MIN_HEIGHT,
                            Math.min(
                              DETAIL_PANE_MAX_HEIGHT,
                              height + (e.key === "ArrowUp" ? 32 : -32),
                            ),
                          ),
                  );
                }}
                onMouseDown={handleMouseDown}
                className={`h-3 cursor-ns-resize shrink-0 bg-divider hover:bg-primary transition-colors ${isDragging ? "bg-primary" : ""}`}
                title="Drag to resize"
              >
                <div className="h-full w-12 mx-auto flex items-center justify-center">
                  <div className="w-8 h-0.5 bg-secondary rounded-full" />
                </div>
              </div>
              <div
                style={{
                  height: detailPaneHeight,
                  maxHeight: "min(28dvh, 40%)",
                }}
                className="shrink-0"
              >
                <DetailPane />
              </div>
            </>
          )}
        </>
      )}
    </div>
  );
};
