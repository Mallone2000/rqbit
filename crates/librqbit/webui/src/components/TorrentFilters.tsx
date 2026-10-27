import { useEffect, useMemo, useState } from "react";
import debounce from "lodash.debounce";
import { GoSearch, GoX } from "react-icons/go";
import { useUIStore } from "../stores/uiStore";
import { STATUS_FILTER_LABELS, StatusFilter } from "../helper/torrentFilters";

export const TorrentFilters = () => {
  const search = useUIStore((s) => s.searchQuery);
  const setSearch = useUIStore((s) => s.setSearchQuery);
  const status = useUIStore((s) => s.statusFilter);
  const setStatus = useUIStore((s) => s.setStatusFilter);
  const [draft, setDraft] = useState(search);
  const update = useMemo(() => debounce(setSearch, 150), [setSearch]);
  useEffect(() => () => update.cancel(), [update]);
  useEffect(() => {
    setDraft(search);
  }, [search]);
  const clear = () => {
    update.cancel();
    setDraft("");
    setSearch("");
  };
  return (
    <div className="flex shrink-0 flex-wrap items-center gap-3">
      <h2 className="mr-auto text-2xl font-bold tracking-tight">Torrents</h2>
      <div className="relative w-full sm:w-80">
        <GoSearch
          aria-hidden
          className="absolute left-4 top-1/2 -translate-y-1/2 text-secondary"
        />
        <input
          aria-label="Search torrents"
          data-search-input
          placeholder="Search torrents…"
          value={draft}
          onChange={(e) => {
            setDraft(e.target.value);
            update(e.target.value);
          }}
          className="w-full min-h-12 border border-divider bg-surface pl-11 pr-12 py-3"
        />
        {draft && (
          <button
            type="button"
            aria-label="Clear search"
            onClick={clear}
            className="absolute right-0 top-1/2 -translate-y-1/2 w-11 flex items-center justify-center"
          >
            <GoX />
          </button>
        )}
      </div>
      <select
        aria-label="Filter torrents by status"
        value={status}
        onChange={(e) => setStatus(e.target.value as StatusFilter)}
        className="min-h-12 border border-divider bg-surface px-4 py-3 grow sm:grow-0"
      >
        {(Object.keys(STATUS_FILTER_LABELS) as StatusFilter[]).map((key) => (
          <option key={key} value={key}>
            {STATUS_FILTER_LABELS[key]}
          </option>
        ))}
      </select>
    </div>
  );
};
