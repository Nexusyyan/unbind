import { Package, Power } from "lucide-react";
import type { Dict } from "../i18n";
import type { PortableApp, StartupItem, ToolsSubTab } from "../types";
import { PortableAppsView } from "./PortableAppsView";
import { StartupManagerView } from "./StartupManagerView";

interface ToolsViewProps {
  dict: Dict;
  subTab: ToolsSubTab;
  onChangeSubTab: (tab: ToolsSubTab) => void;
  startupItems: StartupItem[];
  startupLoading: boolean;
  onRefreshStartup: () => void;
  portableRoots: string[];
  portableApps: PortableApp[];
  portableLoading: boolean;
  portableIcons: Record<string, string>;
  onAddPortableRoot: () => Promise<void>;
  onRemovePortableRoot: (root: string) => Promise<void>;
  onRefreshPortable: () => void;
  onDeepRemovePortable: (targetPath: string) => Promise<void>;
  onNotify: (kind: "info" | "success" | "warning" | "error", title: string, detail?: string) => void;
}

export function ToolsView({
  dict,
  subTab,
  onChangeSubTab,
  startupItems,
  startupLoading,
  onRefreshStartup,
  portableRoots,
  portableApps,
  portableLoading,
  portableIcons,
  onAddPortableRoot,
  onRemovePortableRoot,
  onRefreshPortable,
  onDeepRemovePortable,
  onNotify,
}: ToolsViewProps) {
  const enabledStartupCount = startupItems.filter((s) => s.enabled).length;

  return (
    <div className="flex h-full min-h-0 flex-1 flex-col bg-[var(--bg-canvas)]">
      {/* Unified Header + Minimal Flat Secondary Tabs */}
      <div className="space-y-3 border-b border-[var(--border-hairline)] px-6 pt-5 pb-3">
        <div>
          <h1 className="text-[17px] font-medium tracking-[-0.015em] text-[var(--text-primary)]">
            {dict.tools.title}
          </h1>
          <p className="mt-0.5 text-[12.5px] text-[var(--text-secondary)]">
            {dict.tools.subtitle}
          </p>
        </div>

        <div className="flex items-center gap-1">
          <button
            type="button"
            onClick={() => onChangeSubTab("startup")}
            className={`inline-flex items-center gap-2 rounded-[8px] px-3 py-1.5 text-[12.5px] transition-colors duration-150 cursor-pointer ${
              subTab === "startup"
                ? "bg-[var(--bg-selected)] font-medium text-[var(--text-primary)]"
                : "text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]"
            }`}
          >
            <Power className="h-3.5 w-3.5" strokeWidth={1.5} />
            <span>{dict.tools.tabStartup}</span>
            <span className="num-tabular text-[11.5px] text-[var(--text-tertiary)]">
              {enabledStartupCount}
            </span>
          </button>

          <button
            type="button"
            onClick={() => onChangeSubTab("portable")}
            className={`inline-flex items-center gap-2 rounded-[8px] px-3 py-1.5 text-[12.5px] transition-colors duration-150 cursor-pointer ${
              subTab === "portable"
                ? "bg-[var(--bg-selected)] font-medium text-[var(--text-primary)]"
                : "text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]"
            }`}
          >
            <Package className="h-3.5 w-3.5" strokeWidth={1.5} />
            <span>{dict.tools.tabPortable}</span>
            <span className="num-tabular text-[11.5px] text-[var(--text-tertiary)]">
              {portableApps.length}
            </span>
          </button>
        </div>
      </div>

      {subTab === "startup" ? (
        <StartupManagerView
          dict={dict}
          items={startupItems}
          loading={startupLoading}
          embedded
          onRefresh={onRefreshStartup}
          onNotify={onNotify}
        />
      ) : (
        <PortableAppsView
          dict={dict}
          roots={portableRoots}
          apps={portableApps}
          loading={portableLoading}
          icons={portableIcons}
          onAddRoot={onAddPortableRoot}
          onRemoveRoot={onRemovePortableRoot}
          onRefresh={onRefreshPortable}
          onDeepRemove={onDeepRemovePortable}
          onNotify={onNotify}
        />
      )}
    </div>
  );
}
