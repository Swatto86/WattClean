// The pages each platform offers, in the navigation's order. The registry
// cleaner and the registry-based uninstaller are Windows' only; on Linux
// and macOS "Uninstall programs" lists what the package managers installed.

import type { Context, Page } from "./context.ts";
import { el, type GlyphName } from "./dom.ts";
import { cleanerPage } from "./pages/cleaner.ts";
import { historyPage } from "./pages/history.ts";
import { programsPage } from "./packages/page.ts";
import { settingsPage } from "./pages/settings.ts";
import { shredPage } from "./pages/shred.ts";
import { wipePage } from "./pages/wipe.ts";
import type { ModuleView, RegistryViews } from "./registry/start.ts";
import type { UninstallViews } from "./uninstall/start.ts";

export interface PageEntry {
  id: string;
  label: string;
  glyph: GlyphName;
  make: (ctx: Context) => Page;
  /** The module heading shown above it in the navigation. */
  group?: string;
  bottom?: boolean;
}

/**
 * A module's view as a page: it keeps its own toolbar, and scrolls inside
 * a page-sized frame (as in the app it came from) rather than being the
 * frame, so its lists grow instead of shrinking to fit.
 */
const modulePage = (view: ModuleView) => (): Page => {
  view.element.hidden = false;
  view.element.classList.add("module-view");
  return {
    element: el("div", { class: "page module-page" }, view.element),
    commands: [],
    ...(view.onShow ? { onShow: view.onShow } : {}),
  };
};

/** The disk cleaner's pages, on every platform. */
const diskPages: PageEntry[] = [
  {
    id: "cleaner",
    label: "Disk cleaner",
    glyph: "cleaner",
    group: "Disk",
    make: cleanerPage,
  },
  {
    id: "shred",
    label: "Shred files",
    glyph: "shred",
    group: "Disk",
    make: shredPage,
  },
  {
    id: "wipe",
    label: "Wipe free space",
    glyph: "wipe",
    group: "Disk",
    make: wipePage,
  },
  {
    id: "history",
    label: "Clean history",
    glyph: "history",
    group: "Disk",
    make: historyPage,
  },
];

const settingsEntry: PageEntry = {
  id: "settings",
  label: "Settings",
  glyph: "settings",
  make: settingsPage,
  bottom: true,
};

/** Every page on Windows, in the navigation's order. */
export const windowsPages = (
  registry: RegistryViews,
  uninstall: UninstallViews,
): PageEntry[] => [
  ...diskPages,
  {
    id: "registry",
    label: "Registry cleaner",
    glyph: "registry",
    group: "Registry",
    make: modulePage(registry.cleaner),
  },
  {
    id: "regsearch",
    label: "Registry search",
    glyph: "search",
    group: "Registry",
    make: modulePage(registry.search),
  },
  {
    id: "tuner",
    label: "Registry tuner",
    glyph: "tuner",
    group: "Registry",
    make: modulePage(registry.tuner),
  },
  {
    id: "watcher",
    label: "Registry watcher",
    glyph: "watcher",
    group: "Registry",
    make: modulePage(registry.watcher),
  },
  {
    id: "regbackups",
    label: "Registry backups",
    glyph: "backup",
    group: "Registry",
    make: modulePage(registry.backups),
  },
  {
    id: "schedules",
    label: "Schedules",
    glyph: "schedule",
    group: "Registry",
    make: modulePage(registry.schedules),
  },
  {
    id: "programs",
    label: "Uninstall programs",
    glyph: "programs",
    group: "Programs",
    make: modulePage(uninstall.programs),
  },
  {
    id: "traced",
    label: "Traced installs",
    glyph: "trace",
    group: "Programs",
    make: modulePage(uninstall.traced),
  },
  {
    id: "forced",
    label: "Forced uninstall",
    glyph: "forced",
    group: "Programs",
    make: modulePage(uninstall.forced),
  },
  {
    id: "unbackups",
    label: "Uninstall backups",
    glyph: "backup",
    group: "Programs",
    make: modulePage(uninstall.backups),
  },
  {
    id: "unhistory",
    label: "Uninstall history",
    glyph: "history",
    group: "Programs",
    make: modulePage(uninstall.history),
  },
  settingsEntry,
];

/** Every page on Linux and macOS. */
export const unixPages = (): PageEntry[] => [
  ...diskPages,
  {
    id: "programs",
    label: "Uninstall programs",
    glyph: "programs",
    group: "Programs",
    make: programsPage,
  },
  settingsEntry,
];
