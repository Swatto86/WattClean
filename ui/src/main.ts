// The shell: navigation grouped by module, header, status bar, theme, the
// notification-area icon's requests, and showing the window once the first
// page has painted (so it never flashes white). The registry cleaner's and
// uninstaller's views are wired at start and shown here as pages.

import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

import {
  api,
  type AppInfo,
  type Settings,
  type SettingsPatch,
  type ShellSettings,
} from "./api.ts";
import { type Context, type Page } from "./context.ts";
import { el, glyph, notice, setText, type GlyphName } from "./dom.ts";
import { cleanerPage } from "./pages/cleaner.ts";
import { historyPage } from "./pages/history.ts";
import { settingsPage } from "./pages/settings.ts";
import { shredPage } from "./pages/shred.ts";
import { wipePage } from "./pages/wipe.ts";
import {
  startRegistry,
  type ModuleView,
  type RegistryViews,
} from "./registry/start.ts";
import { applyTheme, onSystemThemeChange } from "./theme.ts";
import { startUninstall, type UninstallViews } from "./uninstall/start.ts";

interface PageEntry {
  id: string;
  label: string;
  glyph: GlyphName;
  make: (ctx: Context) => Page;
  /** The module heading shown above it in the navigation. */
  group?: string;
  bottom?: boolean;
}

const byId = (id: string): HTMLElement => {
  const node = document.getElementById(id);
  if (!node) throw new Error(`#${id} is missing from index.html`);
  return node;
};

const inputById = (id: string): HTMLInputElement => {
  const node = byId(id);
  if (!(node instanceof HTMLInputElement))
    throw new Error(`#${id} is not a text box`);
  return node;
};

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

/** Every page, in the navigation's order. */
const pageList = (
  registry: RegistryViews,
  uninstall: UninstallViews,
): PageEntry[] => [
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
  {
    id: "settings",
    label: "Settings",
    glyph: "settings",
    make: settingsPage,
    bottom: true,
  },
];

/** Show the window once the page has painted, so it never flashes white. */
const reveal = async (): Promise<void> => {
  await new Promise((resolve) => requestAnimationFrame(resolve));
  await getCurrentWindow()
    .show()
    .catch(() => undefined);
};

const applyLook = (shell: ShellSettings): void => {
  applyTheme(shell.theme, shell.accent);
  document.documentElement.dataset["textSize"] = shell.textSize;
};

const boot = async (): Promise<void> => {
  const [info, first, firstShell] = await Promise.all([
    api.appInfo(),
    api.getSettings(),
    api.getShellSettings(),
  ]);
  let settings: Settings = first;
  let shell: ShellSettings = firstShell;
  const listeners: ((s: Settings) => void)[] = [];
  applyLook(shell);
  onSystemThemeChange(() => applyLook(shell));

  const restartAsAdmin = () => {
    api.restartAsAdmin().catch((e: unknown) => notice(String(e), true));
  };
  const host = { restartAsAdmin, reveal };
  const registry = await startRegistry(host);
  const uninstall = await startUninstall(host);

  const ctx: Context = {
    info,
    settings: () => settings,
    update: async (patch: SettingsPatch) => {
      settings = await api.updateSettings(patch);
      for (const listener of listeners) listener(settings);
      return settings;
    },
    onSettings: (listener) => listeners.push(listener),
    shell: () => shell,
    updateShell: async (patch) => {
      shell = await api.updateShellSettings(patch);
      applyLook(shell);
      return shell;
    },
    sections: [
      {
        id: "registry",
        title: "Registry cleaner",
        element: registry.settings.element,
        ...(registry.settings.onShow
          ? { onShow: registry.settings.onShow }
          : {}),
      },
      {
        id: "uninstall",
        title: "Uninstaller",
        element: uninstall.settings.element,
      },
    ],
    startWithWindows: document.getElementById("reg-background-card"),
    setSelectionStatus: (text) => setText(byId("status-selection"), text),
    go: (id) => show(id),
  };

  const pages = pageList(registry, uninstall);
  const content = byId("content");
  const commands = byId("commands");
  const title = byId("page-title");
  const searchBox = byId("search-box");
  const search = inputById("search");
  const made = new Map<string, Page>();
  const buttons = new Map<string, HTMLButtonElement>();
  let current = "";

  // Every page stays in the document once made (the modules' views keep
  // working while another page is shown); only the current one shows.
  const show = (id: string): void => {
    const entry = pages.find((p) => p.id === id);
    if (!entry || id === current) return;
    current = id;
    let page = made.get(id);
    if (!page) {
      page = entry.make(ctx);
      made.set(id, page);
      content.append(page.element);
    }
    for (const [other, shown] of made) shown.element.hidden = other !== id;
    commands.replaceChildren(...page.commands);
    // The ticked-options count belongs to the disk cleaner alone.
    byId("status-selection").hidden = id !== "cleaner";
    title.textContent = entry.label;
    document.documentElement.style.setProperty("--page", `var(--page-${id})`);
    searchBox.hidden = !page.onSearch;
    for (const [other, button] of buttons) {
      if (other === id) button.setAttribute("aria-current", "page");
      else button.removeAttribute("aria-current");
    }
    page.onShow?.();
  };

  search.addEventListener("input", () =>
    made.get(current)?.onSearch?.(search.value),
  );

  const navPages = byId("nav-pages");
  const navBottom = byId("nav-bottom");
  let group = "";
  for (const entry of pages) {
    if (entry.group && entry.group !== group) {
      group = entry.group;
      navPages.append(el("div", { class: "nav-group", text: group }));
    }
    const button = el(
      "button",
      {
        id: `nav-${entry.id}`,
        class: "nav-item",
        type: "button",
        title: entry.label,
      },
      glyph(entry.glyph),
      el("span", { class: "nav-label", text: entry.label }),
    );
    button.style.setProperty("--page", `var(--page-${entry.id})`);
    button.addEventListener("click", () => show(entry.id));
    buttons.set(entry.id, button);
    (entry.bottom ? navBottom : navPages).append(button);
  }

  // Up and Down move between pages while the navigation has the focus, as
  // the old apps' section tabs did.
  const order = pages.map((p) => p.id);
  byId("nav").addEventListener("keydown", (event) => {
    const step =
      event.key === "ArrowDown" ? 1 : event.key === "ArrowUp" ? -1 : 0;
    if (step === 0) return;
    event.preventDefault();
    const at = order.indexOf(current);
    const next = order[(at + step + order.length) % order.length];
    if (next === undefined) return;
    show(next);
    buttons.get(next)?.focus();
  });

  const app = byId("app");
  const toggle = byId("nav-toggle");
  const collapse = (collapsed: boolean) => {
    app.classList.toggle("nav-collapsed", collapsed);
    toggle.setAttribute("aria-expanded", String(!collapsed));
  };
  collapse(shell.navCollapsed);
  toggle.addEventListener("click", () => {
    const collapsed = !app.classList.contains("nav-collapsed");
    collapse(collapsed);
    ctx
      .updateShell({ navCollapsed: collapsed })
      .catch((e: unknown) => notice(String(e), true));
  });

  // The notification-area icon's menu opens a page.
  await listen<string>("open-page", (event) => show(event.payload));

  statusBar(info, restartAsAdmin);
  byId("test-machine").hidden = !info.testMachine;
  show("cleaner");
  // Started at sign-in: stay in the notification area until opened.
  if (!info.startedHidden) await reveal();
};

const statusBar = (info: AppInfo, restartAsAdmin: () => void): void => {
  setText(
    byId("status-rights"),
    info.elevated ? "Administrator" : "Standard user",
  );
  const admin = byId("status-admin");
  admin.hidden = info.elevated;
  admin.addEventListener("click", restartAsAdmin);
  setText(byId("status-note"), `WattClean ${info.version}`);
};

boot().catch(async (error: unknown) => {
  document.body.textContent = `WattClean could not start: ${String(error)}`;
  await getCurrentWindow().show();
});
