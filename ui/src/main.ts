// The shell: navigation grouped by module, header, status bar, theme, the
// notification-area icon's requests, and showing the window once the first
// page has painted (so it never flashes white). On Windows the registry
// cleaner's and uninstaller's views are wired at start and shown here as
// pages; Linux and macOS have no registry (see pagelist.ts).

import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

import {
  api,
  type AppInfo,
  type Settings,
  type SettingsPatch,
  type ShellSettings,
} from "./api.ts";
import { type Context, type Page, type SettingsSection } from "./context.ts";
import { dialogsWork, inform } from "./dialog.ts";
import { el, glyph, notice, setText } from "./dom.ts";
import { unixPages, windowsPages } from "./pagelist.ts";
import {
  isWindows,
  setPlatform,
  testBuild,
  webEngineTooOld,
} from "./platform.ts";
import { startRegistry } from "./registry/start.ts";
import { applyTheme, onSystemThemeChange } from "./theme.ts";
import { startUninstall } from "./uninstall/start.ts";

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

/** Show the window once the page has painted, so it never flashes white. */
const reveal = async (): Promise<void> => {
  // A hidden WKWebView (macOS) draws no frames until it is shown, so a short
  // timer stands in for the first frame there.
  await Promise.race([
    new Promise<void>((resolve) => requestAnimationFrame(() => resolve())),
    new Promise<void>((resolve) => setTimeout(resolve, 120)),
  ]);
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
  setPlatform(info.platform);
  let settings: Settings = first;
  let shell: ShellSettings = firstShell;
  const listeners: ((s: Settings) => void)[] = [];
  applyLook(shell);
  onSystemThemeChange(() => applyLook(shell));
  // An older WebKit (macOS before 12.3) has no <dialog>: every Clean,
  // Shred, Wipe and Uninstall would silently do nothing.
  if (!dialogsWork()) {
    document.body.textContent = webEngineTooOld();
    await reveal();
    return;
  }

  const restartAsAdmin = () => {
    api.restartAsAdmin().catch((e: unknown) => notice(String(e), true));
  };
  const host = { restartAsAdmin, reveal };
  const modules = isWindows()
    ? {
        registry: await startRegistry(host),
        uninstall: await startUninstall(host),
      }
    : null;
  const sections: SettingsSection[] = modules
    ? [
        {
          id: "registry",
          title: "Registry cleaner",
          element: modules.registry.settings.element,
          ...(modules.registry.settings.onShow
            ? { onShow: modules.registry.settings.onShow }
            : {}),
        },
        {
          id: "uninstall",
          title: "Uninstaller",
          element: modules.uninstall.settings.element,
        },
      ]
    : [];

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
    sections,
    startWithWindows: modules
      ? document.getElementById("reg-background-card")
      : null,
    setSelectionStatus: (text) => setText(byId("status-selection"), text),
    go: (id) => show(id),
  };

  const pages = modules
    ? windowsPages(modules.registry, modules.uninstall)
    : unixPages();
  const content = byId("content");
  const commands = byId("commands");
  const title = byId("page-title");
  const searchBox = byId("search-box");
  const search = inputById("search");
  const made = new Map<string, Page>();
  const queries = new Map<string, string>();
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
    // Each page keeps what was typed in its own search box.
    searchBox.hidden = !page.search;
    if (page.search) {
      search.placeholder = page.search.placeholder;
      search.setAttribute("aria-label", page.search.placeholder);
      search.value = queries.get(id) ?? "";
    }
    for (const [other, button] of buttons) {
      if (other === id) button.setAttribute("aria-current", "page");
      else button.removeAttribute("aria-current");
    }
    page.onShow?.();
  };

  search.addEventListener("input", () => {
    queries.set(current, search.value);
    made.get(current)?.search?.run(search.value);
  });

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
    // A page's name pops up only when the narrow navigation hides it; over
    // a name already shown it would cover the next one.
    for (const entry of pages) {
      const button = buttons.get(entry.id);
      if (collapsed) button?.setAttribute("title", entry.label);
      else button?.removeAttribute("title");
    }
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
  const pill = byId("test-machine");
  pill.hidden = !info.testMachine;
  const test = testBuild();
  pill.textContent = test.label;
  pill.title = test.title;
  show("cleaner");
  // Started at sign-in: stay in the notification area until opened.
  if (!info.startedHidden) await reveal();
  if (info.adoptProblems.length > 0) {
    void inform(
      "Some earlier data was not copied",
      "WattClean copies the data of the apps it replaces. These could not be copied; WattClean tries again at its next start.",
      info.adoptProblems,
    );
  }
};

const statusBar = (info: AppInfo, restartAsAdmin: () => void): void => {
  setText(
    byId("status-rights"),
    info.elevated ? "Administrator" : "Standard user",
  );
  const admin = byId("status-admin");
  // Linux and macOS ask for administrator rights per task instead.
  admin.hidden = info.elevated || !isWindows();
  admin.addEventListener("click", restartAsAdmin);
  setText(byId("status-note"), `WattClean ${info.version}`);
};

boot().catch(async (error: unknown) => {
  document.body.textContent = `WattClean could not start: ${String(error)}`;
  await getCurrentWindow().show();
});
