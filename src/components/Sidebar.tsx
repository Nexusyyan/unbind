import {
  Activity,
  ArchiveRestore,
  Layers,
  Power,
  Settings,
  ShoppingBag,
} from "lucide-react";
import type { Dict } from "../i18n";
import type { NavTab } from "../types";
import { formatSizeKb } from "../utils/format";

interface SidebarProps {
  dict: Dict;
  activeTab: NavTab;
  onSelectTab: (tab: NavTab) => void;
  counts: {
    apps: number;
    store: number;
    startup: number;
    traces: number;
    activeTracks: number;
    backups: number;
  };
  totalSizeKb: number;
}

export function Sidebar({
  dict,
  activeTab,
  onSelectTab,
  counts,
  totalSizeKb,
}: SidebarProps) {
  const mainItems: Array<{
    id: NavTab;
    label: string;
    icon: typeof Layers;
    badge?: number;
    pulse?: boolean;
  }> = [
    { id: "apps", label: dict.nav.apps, icon: Layers, badge: counts.apps },
    { id: "store", label: dict.nav.store, icon: ShoppingBag, badge: counts.store },
    { id: "startup", label: dict.nav.startup, icon: Power, badge: counts.startup },
    {
      id: "tracker",
      label: dict.nav.tracker,
      icon: Activity,
      badge: counts.activeTracks > 0 ? counts.activeTracks : counts.traces,
      pulse: counts.activeTracks > 0,
    },
    { id: "backups", label: dict.nav.backups, icon: ArchiveRestore, badge: counts.backups },
  ];

  const renderNavButton = (item: {
    id: NavTab;
    label: string;
    icon: typeof Layers;
    badge?: number;
    pulse?: boolean;
  }) => {
    const Icon = item.icon;
    const active = activeTab === item.id;
    return (
      <button
        key={item.id}
        type="button"
        onClick={() => onSelectTab(item.id)}
        className={`group flex w-full items-center justify-between rounded-[9px] px-2.5 py-[7px] text-left text-[13px] transition-colors duration-150 cursor-pointer ${
          active
            ? "bg-[var(--bg-selected)] font-medium text-[var(--text-primary)]"
            : "font-normal text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]"
        }`}
      >
        <span className="flex min-w-0 items-center gap-2.5">
          <Icon
            className={`h-[15px] w-[15px] shrink-0 transition-colors duration-150 ${
              active
                ? "text-[var(--text-primary)]"
                : "text-[var(--text-tertiary)] group-hover:text-[var(--text-secondary)]"
            }`}
            strokeWidth={1.5}
          />
          <span className="truncate">{item.label}</span>
        </span>
        {item.badge !== undefined && item.badge > 0 && (
          <span className="ml-2 flex shrink-0 items-center gap-1.5">
            {item.pulse && (
              <span className="h-1.5 w-1.5 rounded-full bg-[var(--success-text)] animate-pulse" />
            )}
            <span
              className={`num-tabular text-[11.5px] ${
                active ? "text-[var(--text-secondary)]" : "text-[var(--text-tertiary)]"
              }`}
            >
              {item.badge}
            </span>
          </span>
        )}
      </button>
    );
  };

  return (
    <aside className="flex w-[222px] shrink-0 flex-col justify-between bg-[var(--bg-sidebar)] px-2.5 pt-2 pb-3 select-none">
      <nav className="flex flex-col gap-0.5">
        {mainItems.map(renderNavButton)}
      </nav>

      <div className="pt-2">
        {renderNavButton({ id: "settings", label: dict.nav.settings, icon: Settings })}

        <div className="mx-1.5 my-2.5 h-px bg-[var(--border-hairline)]" />

        {/* Integrated minimal text-only storage summary */}
        <div className="px-2.5">
          <div className="flex items-baseline justify-between gap-2">
            <span className="text-[11px] text-[var(--text-tertiary)]">
              {dict.status.totalSize}
            </span>
            <span className="num-tabular text-[12px] font-medium text-[var(--text-primary)]">
              {formatSizeKb(totalSizeKb)}
            </span>
          </div>
          <div className="num-tabular mt-0.5 text-[11px] text-[var(--text-tertiary)]">
            {counts.apps + counts.store} {dict.status.programs}
          </div>

          <p className="mt-2.5 text-[10.5px] leading-[1.45] text-[var(--text-tertiary)]">
            {dict.appList.dropHint}
          </p>
        </div>
      </div>
    </aside>
  );
}
