/**
 * The registry cleaner inside WattClean: its views are in index.html (ids
 * prefixed `reg-`), wired once at start-up here, and shown by the shell as
 * pages. The look, the tray and quitting are the shell's.
 */
import {
  api,
  message,
  type Category,
  type CategoryInfo,
  type Settings,
} from "./api.ts";
import { wireBackups } from "./backups.ts";
import { wireCleaner } from "./cleaner.ts";
import type { AppContext } from "./context.ts";
import { inform } from "./dialog.ts";
import { byId } from "./dom.ts";
import { installMenus } from "./menu.ts";
import { wireSchedules } from "./schedules.ts";
import { wireSearch } from "./search.ts";
import { wireSettings } from "./settings.ts";
import { wireTuner } from "./tuner.ts";
import { wireWatcher } from "./watcher.ts";

/** A view the shell shows as a page. */
export interface ModuleView {
  element: HTMLElement;
  /** Called each time the page is opened. */
  onShow?: () => void;
}

export interface RegistryViews {
  cleaner: ModuleView;
  search: ModuleView;
  tuner: ModuleView;
  watcher: ModuleView;
  backups: ModuleView;
  schedules: ModuleView;
  settings: ModuleView;
}

/** What the shell lends the module. */
export interface Host {
  restartAsAdmin: () => void;
  /** Shows the window, so a problem at start-up can be read. */
  reveal: () => Promise<void>;
}

const DEFAULTS: Settings = {
  version: 1,
  theme: "system",
  mode: "safe",
  custom: [],
  allAccounts: false,
  excludeRemovable: true,
  excludeWindows: true,
  restorePoint: true,
  forceDelete: false,
  fullBackup: false,
  fontSize: "normal",
  backupsFolder: null,
  keepInTray: false,
  keepBackupsDays: 0,
  keepBackupsMb: 0,
  excludes: [],
  includes: [],
};

async function loadSettings(host: Host): Promise<Settings> {
  try {
    return await api.getSettings();
  } catch (error: unknown) {
    await host.reveal();
    await inform(
      "The registry cleaner's settings could not be read",
      `${message(error)}\n\nThe defaults are used until the file is fixed or removed.`,
    );
    return DEFAULTS;
  }
}

export async function startRegistry(host: Host): Promise<RegistryViews> {
  // One right-click handler for the whole window: Windows' edit menu on
  // text, a list's own menu on a list.
  installMenus();
  let settings = await loadSettings(host);
  const [info, categories] = await Promise.all([
    api.appInfo(),
    api.categories(),
  ]);
  const titles = new Map<Category, CategoryInfo>(
    categories.map((category) => [category.id, category]),
  );
  let refreshSettings: () => void = () => {};
  let refreshBackups: () => void = () => {};
  const context: AppContext = {
    info,
    titles,
    settings: () => settings,
    update: async (change) => {
      const next = change(settings);
      await api.saveSettings(next);
      settings = next;
      refreshSettings();
    },
    restartAsAdmin: host.restartAsAdmin,
    backupsChanged: () => refreshBackups(),
  };
  wireCleaner(context);
  wireSearch(context);
  const tuner = wireTuner();
  const watcher = wireWatcher(context);
  const backups = wireBackups(context);
  const schedules = wireSchedules(context);
  const settingsView = wireSettings(context);
  refreshSettings = settingsView.refresh;
  refreshBackups = () => void backups.load().catch(() => undefined);
  const loader = (view: { load: () => Promise<void> }) => () =>
    void view
      .load()
      .catch((error: unknown) => inform("Could not load", message(error)));
  const view = (id: string, onShow?: () => void): ModuleView => ({
    element: byId(`view-${id}`, HTMLElement),
    ...(onShow ? { onShow } : {}),
  });
  return {
    cleaner: view("cleaner"),
    search: view("search"),
    tuner: view("tuner", loader(tuner)),
    watcher: view("watcher", loader(watcher)),
    backups: view("backups", loader(backups)),
    schedules: view("schedules", loader(schedules)),
    settings: view("settings", settingsView.refresh),
  };
}
