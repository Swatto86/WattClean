/**
 * The uninstaller inside WattClean: its views are in index.html (ids
 * prefixed `un-`), wired once at start-up here, and shown by the shell as
 * pages. The look and right-click menus are the shell's.
 */
import { api, MODES, type Settings } from "./api.ts";
import { uninstallMany } from "./batch.ts";
import { inform } from "./dialog.ts";
import { byId } from "./dom.ts";
import { ProgramList } from "./programs.ts";
import { showBackups, showHistory, wireHistory } from "./records.ts";
import { wireSettings } from "./settings.ts";
import { installWithTracing, showLogs } from "./traced.ts";
import { forced, uninstall, type WizardContext } from "./wizard.ts";

const message = (error: unknown) =>
  error instanceof Error ? error.message : String(error);

/** A view the shell shows as a page. */
export interface ModuleView {
  element: HTMLElement;
  /** Called each time the page is opened. */
  onShow?: () => void;
}

export interface UninstallViews {
  programs: ModuleView;
  traced: ModuleView;
  forced: ModuleView;
  backups: ModuleView;
  history: ModuleView;
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
  mode: "moderate",
  restorePoint: true,
  showSystemComponents: false,
  quiet: false,
  theme: "system",
};

export async function startUninstall(host: Host): Promise<UninstallViews> {
  let settings: Settings;
  try {
    settings = await api.getSettings();
  } catch (error: unknown) {
    await host.reveal();
    await inform(
      "The uninstaller's settings could not be read",
      `${message(error)}\n\nThe defaults are used until the file is fixed or removed.`,
    );
    settings = DEFAULTS;
  }

  let programs: ProgramList | null = null;
  // One uninstall at a time: a held Enter or a second click on Uninstall
  // must not open a second one for the same programs.
  let open = false;
  const context = (): WizardContext => ({
    settings,
    restartAsAdmin: host.restartAsAdmin,
    onDone: () => {
      open = false;
      void programs?.load();
    },
  });
  const begin = (start: () => Promise<void>): Promise<void> => {
    if (open) return Promise.resolve();
    open = true;
    return start().catch((error: unknown) => {
      open = false;
      throw error;
    });
  };
  programs = new ProgramList((chosen) => {
    const [program] = chosen;
    if (!program) return;
    begin(() =>
      chosen.length > 1
        ? uninstallMany(chosen, context())
        : uninstall(program, context()),
    ).catch((error: unknown) =>
      inform("Could not start the uninstall", message(error)),
    );
  });
  wireSettings(settings, (next) => {
    const listChanged =
      next.showSystemComponents !== settings.showSystemComponents;
    settings = next;
    if (listChanged) void programs?.load();
  });
  wireHistory();
  byId("trace-install", HTMLButtonElement).addEventListener("click", () => {
    void installWithTracing(() => {
      void showLogs();
      void programs?.load();
    });
  });
  wireForced(() => settings, context, begin);
  const list = programs;
  const programsView = byId("view-programs", HTMLElement);
  // F5 refreshes the list while it is shown, as Revo's does.
  document.addEventListener("keydown", (event) => {
    if (event.key === "F5" && programsView.checkVisibility()) {
      event.preventDefault();
      void list.load();
    }
  });
  let loaded = false;
  return {
    programs: {
      element: programsView,
      onShow: () => {
        if (!loaded) {
          loaded = true;
          void list.load();
        }
      },
    },
    traced: {
      element: byId("view-traced", HTMLElement),
      onShow: () => void showLogs(),
    },
    forced: { element: byId("view-forced", HTMLElement) },
    backups: {
      element: byId("view-backups", HTMLElement),
      onShow: () => void showBackups(),
    },
    history: {
      element: byId("view-history", HTMLElement),
      onShow: () => void showHistory(),
    },
    settings: { element: byId("view-settings", HTMLElement) },
  };
}

function wireForced(
  current: () => Settings,
  context: () => WizardContext,
  begin: (start: () => Promise<void>) => Promise<void>,
): void {
  const form = byId("forced-form", HTMLFormElement);
  const error = byId("forced-error", HTMLParagraphElement);
  byId("forced-mode", HTMLSelectElement).value = current().mode;
  form.addEventListener("submit", (event) => {
    event.preventDefault();
    error.textContent = "";
    const name = byId("forced-name", HTMLInputElement).value;
    const folder = byId("forced-folder", HTMLInputElement).value;
    const picked = byId("forced-mode", HTMLSelectElement).value;
    const mode = MODES.find((value) => value === picked) ?? "moderate";
    begin(() => forced(name, folder, mode, context())).catch(
      (failure: unknown) => {
        error.textContent = message(failure);
      },
    );
  });
}
