import { QueueConfig } from "../../api-types";
import { FormCheckbox } from "../forms/FormCheckbox";
import { Fieldset } from "../forms/Fieldset";

const numericFields = [
  [
    "max_active_downloads",
    "Active downloads",
    -1,
    "-1 means unlimited; 0 holds all automatic downloads.",
  ],
  [
    "max_active_uploads",
    "Active seeds",
    -1,
    "-1 means unlimited; 0 holds all automatic seeds.",
  ],
  [
    "max_active_torrents",
    "Total active torrents",
    -1,
    "Includes slow torrents; force-started torrents are exempt.",
  ],
  [
    "slow_torrent_dl_rate_threshold",
    "Slow download threshold (KiB/s)",
    0,
    "Download speed below which a torrent can be considered slow.",
  ],
  [
    "slow_torrent_ul_rate_threshold",
    "Slow upload threshold (KiB/s)",
    0,
    "Upload speed below which a torrent can be considered slow.",
  ],
  [
    "slow_torrent_inactive_timer",
    "Startup grace (seconds)",
    0,
    "Newly started torrents count toward limits during this period.",
  ],
] as const;

export const QueueingTab = ({
  config,
  onChange,
}: {
  config: QueueConfig;
  onChange: (config: QueueConfig) => void;
}) => (
  <Fieldset>
    <FormCheckbox
      name="queue_enabled"
      label="Enable torrent queueing"
      checked={config.enabled}
      onChange={(e) => onChange({ ...config, enabled: e.target.checked })}
    />
    <FormCheckbox
      name="ignore_slow"
      label="Exclude slow torrents from download and seed limits"
      checked={config.dont_count_slow_torrents}
      onChange={(e) =>
        onChange({ ...config, dont_count_slow_torrents: e.target.checked })
      }
    />
    {numericFields.map(([key, label, min, help]) => (
      <label key={key} className="flex flex-col gap-1 text-sm">
        {label}
        <input
          type="number"
          min={min}
          max={key.includes("rate_threshold") ? 2097151 : 2147483647}
          step={1}
          required
          value={config[key]}
          onChange={(e) =>
            onChange({ ...config, [key]: e.target.valueAsNumber })
          }
          className="rounded border border-divider bg-surface p-2 text-text"
        />
        <span className="text-secondary">{help}</span>
      </label>
    ))}
    <p className="text-sm text-secondary">
      Downloads take precedence over seeds. Manual pauses remain stopped.
      Settings survive restart unless explicitly overridden at startup.
    </p>
  </Fieldset>
);
