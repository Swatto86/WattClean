/**
 * The registry cleaner's settings, shown in WattClean's Settings page (the
 * look and the tray are the shell's). Every change is saved at once.
 */
import { api, message, type Background, type ListEntry } from "./api.ts";
import type { AppContext } from "./context.ts";
import { inform } from "./dialog.ts";
import { byId, h, PREFIX } from "./dom.ts";
import { copyText } from "./format.ts";
import { item, onMenu, popup, separator } from "./menu.ts";

type ListName = "excludes" | "includes";

export interface SettingsView {
  /** Show the settings as saved now (another view may have changed them). */
  refresh: () => void;
}

export function wireSettings(context: AppContext): SettingsView {
  const removable = byId("set-removable", HTMLInputElement);
  const windows = byId("set-windows", HTMLInputElement);
  const restore = byId("set-restore", HTMLInputElement);
  const full = byId("set-full", HTMLInputElement);
  const force = byId("set-force", HTMLInputElement);
  const keepDays = byId("set-keep-days", HTMLInputElement);
  const keepMb = byId("set-keep-mb", HTMLInputElement);
  const status = byId("settings-status", HTMLParagraphElement);
  const save = async (change: Parameters<AppContext["update"]>[0]) => {
    try {
      await context.update(change);
      status.textContent = "Saved.";
    } catch (error: unknown) {
      status.textContent = `Not saved: ${message(error)}`;
      throw error;
    }
  };

  for (const control of [
    removable,
    windows,
    restore,
    full,
    force,
    keepDays,
    keepMb,
  ]) {
    control.addEventListener("change", () => {
      void save((current) => ({
        ...current,
        excludeRemovable: removable.checked,
        excludeWindows: windows.checked,
        restorePoint: restore.checked,
        fullBackup: full.checked,
        forceDelete: force.checked,
        keepBackupsDays: Math.max(0, Math.trunc(Number(keepDays.value) || 0)),
        keepBackupsMb: Math.max(0, Math.trunc(Number(keepMb.value) || 0)),
      })).catch(() => undefined);
    });
  }

  const setList = (name: ListName, entries: ListEntry[]) =>
    save((current) => ({ ...current, [name]: entries }));

  const renderList = (name: ListName) => {
    const target = byId(name, HTMLDivElement);
    const entries = context.settings()[name];
    const rows = entries.map((entry, index) => {
      const enabled = h("input", {
        type: "checkbox",
        "aria-label": "Use this entry",
      });
      enabled.checked = entry.enabled;
      const subkeys = h("input", { type: "checkbox" });
      subkeys.checked = entry.subkeys;
      const change = (next: Partial<ListEntry>) =>
        void setList(
          name,
          entries.map((old, at) => (at === index ? { ...old, ...next } : old)),
        ).catch(() => undefined);
      enabled.addEventListener("change", () =>
        change({ enabled: enabled.checked }),
      );
      subkeys.addEventListener("change", () =>
        change({ subkeys: subkeys.checked }),
      );
      const remove = h("button", { type: "button", class: "ghost" }, "Remove");
      remove.addEventListener(
        "click",
        () =>
          void setList(
            name,
            entries.filter((_, at) => at !== index),
          ).then(
            () => renderList(name),
            () => undefined,
          ),
      );
      return h(
        "div",
        {
          class: "key-row",
          "data-index": String(index),
          "data-key": entry.key,
        },
        enabled,
        h("code", {}, entry.key),
        h("label", { class: "check" }, subkeys, "Subkeys too"),
        remove,
      );
    });
    const input = h("input", {
      maxlength: "1000",
      placeholder: "HKEY_CURRENT_USER\\Software\\Example",
      "aria-label": `Key to add to the ${name === "excludes" ? "exclude" : "include"} list`,
    });
    const add = h("button", { type: "button", class: "ghost" }, "Add");
    const error = h("p", { class: "error", role: "alert" });
    const submit = () => {
      const key = input.value.trim();
      if (key === "") return;
      setList(name, [...entries, { key, subkeys: false, enabled: true }]).then(
        () => renderList(name),
        (failure: unknown) => {
          error.textContent = message(failure);
        },
      );
    };
    add.addEventListener("click", submit);
    input.addEventListener("keydown", (event) => {
      if (event.key === "Enter") {
        event.preventDefault();
        submit();
      }
    });
    target.replaceChildren(
      ...(rows.length > 0 ? rows : [h("p", { class: "muted" }, "Empty.")]),
      h("div", { class: "key-add" }, input, add),
      error,
    );
  };

  onMenu("keylist", (target, at) => {
    const row = target.closest<HTMLElement>(".key-row");
    const list = target
      .closest<HTMLElement>(".key-list")
      ?.id.slice(PREFIX.length);
    const key = row?.dataset["key"];
    if (!row || !key || (list !== "excludes" && list !== "includes")) return;
    void (async () => {
      const chosen = await popup(
        [
          item("regedit", "&Open in Registry Editor"),
          item("copy", "Cop&y key path"),
          separator,
          item("remove", "&Remove from list"),
        ],
        at,
      );
      try {
        if (chosen === "regedit") await api.openInRegedit(key);
        if (chosen === "copy") await copyText(key);
        if (chosen === "remove") {
          const index = Number(row.dataset["index"]);
          await setList(
            list,
            context.settings()[list].filter((_, at) => at !== index),
          );
          renderList(list);
        }
      } catch (error: unknown) {
        await inform("That did not work", message(error));
      }
    })();
  });

  const refresh = () => {
    const current = context.settings();
    removable.checked = current.excludeRemovable;
    windows.checked = current.excludeWindows;
    restore.checked = current.restorePoint;
    full.checked = current.fullBackup;
    full.disabled = !context.info.elevated;
    byId("backups-folder", HTMLElement).textContent =
      current.backupsFolder ?? "In the data folder (the default)";
    force.checked = current.forceDelete;
    keepDays.value = String(current.keepBackupsDays);
    keepMb.value = String(current.keepBackupsMb);
    renderList("excludes");
    renderList("includes");
  };
  wireBackground(context.info.elevated);
  wireFiles(context, (text) => {
    status.textContent = text;
  });
  refresh();
  return { refresh };
}

/**
 * Revo's export and import of all settings, and its own folder for the
 * backups (moving every backup there).
 */
function wireFiles(context: AppContext, say: (text: string) => void): void {
  const act = (id: string, work: () => Promise<void>) => {
    byId(id, HTMLButtonElement).addEventListener("click", () => {
      void work().catch((error: unknown) =>
        inform("That did not work", message(error)),
      );
    });
  };
  act("settings-export", async () => {
    const saved = await api.exportSettings();
    if (saved) say(`Settings saved to ${saved}`);
  });
  act("settings-import", async () => {
    const imported = await api.importSettings();
    if (!imported) return;
    await context.update(() => imported);
    say("Settings imported.");
  });
  const moved = async (folder: string | null) => {
    if (folder === null) return;
    // Rust saved the new folder; take its settings, not the page's copy.
    const saved = await api.getSettings();
    await context.update(() => saved);
    context.backupsChanged();
    await inform("Backups moved", `Backups are now kept in ${folder}`);
  };
  act("backups-change", async () => moved(await api.chooseBackupsFolder()));
  act("backups-default", async () => moved(await api.defaultBackupsFolder()));
}

/** What starting with Windows does now, or would do if switched on. */
function startNote(state: Background, elevated: boolean): string {
  if (state.startProblem && !state.startWithWindows) {
    return `Start with Windows is unavailable: ${state.startProblem}`;
  }
  if (state.startsElevated) {
    return "Starts with administrator rights, so the Registry Watcher names the program behind each change.";
  }
  if (state.startWithWindows) {
    return elevated
      ? "Starts with ordinary rights, so the Registry Watcher cannot name programs. Switch it off and on again to start with administrator rights."
      : "Starts with ordinary rights, so the Registry Watcher cannot name programs. Switch it on while running as administrator to start with administrator rights.";
  }
  return elevated
    ? "Switched on now, it starts with administrator rights, without asking each time."
    : "Switched on while running as administrator, it starts with administrator rights so the Registry Watcher can name programs.";
}

/** Starting with Windows (only from the installed app). */
function wireBackground(elevated: boolean): void {
  const start = byId("set-autostart", HTMLInputElement);
  const problem = byId("autostart-problem", HTMLParagraphElement);
  const show = (state: Background) => {
    start.checked = state.startWithWindows;
    start.disabled = state.startProblem !== null && !state.startWithWindows;
    problem.textContent = startNote(state, elevated);
  };
  void api.backgroundState().then(show, () => undefined);
  start.addEventListener("change", () => {
    api
      .setStartWithWindows(start.checked)
      .then(show, async (error: unknown) => {
        start.checked = !start.checked;
        await inform("Not changed", message(error));
      });
  });
}
