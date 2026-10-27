import { formatBytes } from "../helper/formatBytes";
import { formatSecondsToTime } from "../helper/formatSecondsToTime";
import { useStatsStore } from "../stores/statsStore";

const FooterPiece: React.FC<{ children: React.ReactNode }> = ({ children }) => {
  return <div className="p-1">{children}</div>;
};

export const Footer: React.FC<{ publicIp?: string | null }> = ({
  publicIp,
}) => {
  let stats = useStatsStore((stats) => stats.stats);
  return (
    <div className="shrink-0 border-t border-divider bg-surface text-sm font-medium text-secondary flex gap-x-4 px-4 py-2 justify-between flex-wrap">
      <FooterPiece>
        ↓ {stats.download_speed.human_readable} (
        {formatBytes(stats.counters.fetched_bytes)})
      </FooterPiece>
      <FooterPiece>
        ↑ {stats.upload_speed.human_readable} (
        {formatBytes(stats.counters.uploaded_bytes)})
      </FooterPiece>
      <FooterPiece>up {formatSecondsToTime(stats.uptime_seconds)}</FooterPiece>
      {publicIp && <FooterPiece>Public IP: {publicIp}</FooterPiece>}
    </div>
  );
};
