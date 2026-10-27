export const ProgressBar = ({
  now,
  label,
  variant = "info",
  classNames = "",
}: {
  now: number;
  label?: string | null;
  variant?: "warn" | "info" | "success" | "error";
  classNames?: string;
}) => {
  const value = Number.isFinite(now) ? Math.min(100, Math.max(0, now)) : 0;
  const colors = {
    warn: "bg-warning-bg",
    info: "bg-primary-bg",
    success: "bg-success-bg",
    error: "bg-error-bg",
  };
  return (
    <div className={classNames}>
      <div className="mb-2 flex justify-between gap-3 text-sm font-semibold">
        <span>{label ?? "Progress"}</span>
        <span className="tabular-nums">{value.toFixed(1)}%</span>
      </div>
      <div
        role="progressbar"
        aria-label={label ?? "Torrent progress"}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={value}
        className="h-2.5 w-full overflow-hidden rounded-full bg-divider"
      >
        <div
          className={`h-full rounded-full ${colors[variant]}`}
          style={{ width: `${value}%` }}
        />
      </div>
    </div>
  );
};
