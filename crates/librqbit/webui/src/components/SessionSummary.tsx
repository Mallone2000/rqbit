import { SessionStats, TorrentListItem } from "../api-types";
import { useMemo } from "react";
import { useStatsStore } from "../stores/statsStore";
import { useTorrentStore } from "../stores/torrentStore";
import { summarizeTorrents } from "../helper/torrentSummary";

export const SessionSummary = () => {
  const stats = useStatsStore((s) => s.stats);
  const loaded = useStatsStore((s) => s.loaded);
  const torrents = useTorrentStore((s) => s.torrents);
  return (
    <SessionSummaryView stats={stats} loaded={loaded} torrents={torrents} />
  );
};

export const SessionSummaryView = ({
  stats,
  loaded,
  torrents,
}: {
  stats: SessionStats;
  loaded: boolean;
  torrents: TorrentListItem[] | null;
}) => {
  const counts = useMemo(() => summarizeTorrents(torrents), [torrents]);
  return (
    <section
      aria-label="Session overview"
      className="grid shrink-0 grid-cols-2 gap-3 sm:grid-cols-4 lg:grid-cols-6"
    >
      {[
        {
          label: "Download",
          value: loaded ? stats.download_speed.human_readable : "—",
          accent: "text-primary",
        },
        {
          label: "Upload",
          value: loaded ? stats.upload_speed.human_readable : "—",
          accent: "text-success",
        },
        ...(["active", "queued", "paused", "completed"] as const).map(
          (key) => ({
            label: key[0].toUpperCase() + key.slice(1),
            value: counts?.[key]?.toLocaleString() ?? "—",
            accent: "text-text",
          }),
        ),
      ].map(({ label, value, accent }) => (
        <div
          key={label}
          className={`rounded-2xl border border-divider bg-surface-raised px-4 py-3 sm:px-5 sm:py-4 lg:px-3 xl:px-5 ${label === "Download" || label === "Upload" ? "sm:col-span-2 lg:col-span-1" : ""}`}
        >
          <div className="text-sm font-semibold text-secondary">{label}</div>
          <div
            className={`stat-value mt-1 text-[28px] leading-tight font-bold break-words ${accent}`}
          >
            {value}
          </div>
          {label === "Completed" && (
            <div className="text-xs text-secondary">
              Includes seeds and pauses
            </div>
          )}
        </div>
      ))}
    </section>
  );
};
