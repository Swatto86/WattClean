/** The Backup Center view: every backup, with restore, export and delete. */
import { api, message, type ItemView, type Summary } from "./api.ts";
import type { AppContext } from "./context.ts";
import { confirm, inform, Modal } from "./dialog.ts";
import { byId, h } from "./dom.ts";
import { count, dateTime } from "./format.ts";
import { item, onMenu, popup, separator } from "./menu.ts";

const KIND_TEXT: Record<Summary["kind"], string> = {
  clean: "Cleaner",
  search: "Search",
  manual: "Hand backup",
  full: "Full registry",
};

/** A full backup is kept as hive files and not put back from here. */
function restorable(backup: Summary): boolean {
  return backup.damaged === null && backup.kind !== "full";
}

export interface BackupsView {
  load: () => Promise<void>;
}

export function wireBackups(context: AppContext): BackupsView {
  const listing = byId("backup-list", HTMLDivElement);
  let backups: Summary[] = [];

  const run = (work: () => Promise<unknown>) =>
    void work()
      .catch((error: unknown) => inform("That did not work", message(error)))
      .then(() => load());

  const restore = async (backup: Summary, items: number[] | null) => {
    const outcome = await api.restoreBackup(backup.id, items);
    const lines = [`${count(outcome.restored, "entry", "entries")} restored.`];
    for (const [what, why] of outcome.skipped.slice(0, 12)) {
      lines.push(`Left as it is: ${what} (${why})`);
    }
    if (outcome.skipped.length > 12) {
      lines.push(`…and ${outcome.skipped.length - 12} more left as they are.`);
    }
    await inform("Restore finished", lines.join("\n"));
  };

  const restoreAll = async (backup: Summary) => {
    const yes = await confirm(
      "Restore backup",
      `Put back the ${count(backup.items, "entry", "entries")} of "${backup.label}"? Anything that exists again is left as it is.`,
      "Restore",
    );
    if (yes) await restore(backup, null);
  };

  /** Save every hive, as Revo's full backup does; it takes a while. */
  const backUpFull = async () => {
    const start = byId("backup-full", HTMLButtonElement);
    start.disabled = true;
    start.textContent = "Backing up…";
    try {
      const done = await api.backUpFull();
      await inform(
        "Full backup made",
        `${done.label}: ${count(done.items, "hive", "hives")} saved.`,
      );
    } finally {
      start.disabled = false;
      start.textContent = "Full backup";
    }
  };

  const exportReg = async (backup: Summary) => {
    const saved = await api.exportBackup(backup.id);
    if (saved) await inform("Backup exported", `Saved to ${saved}`);
  };

  const remove = async (backup: Summary) => {
    const yes = await confirm(
      "Delete backup",
      `Delete "${backup.label}"? Its entries can no longer be restored.`,
      "Delete",
      "danger",
    );
    if (yes) await api.deleteBackup(backup.id);
  };

  const card = (backup: Summary): HTMLElement => {
    const button = (
      label: string,
      work: () => Promise<unknown>,
      kind = "ghost",
    ) => {
      const element = h("button", { type: "button", class: kind }, label);
      element.addEventListener("click", () => run(work));
      return element;
    };
    const facts = backup.damaged
      ? [backup.damaged]
      : [
          dateTime(backup.created),
          count(backup.items, "entry", "entries"),
          backup.kind === "manual" || backup.kind === "full"
            ? ""
            : `${backup.removed} removed`,
          backup.restored > 0 ? `${backup.restored} restored` : "",
          backup.failed > 0 ? `${backup.failed} not removed` : "",
        ];
    return h(
      "article",
      { class: "card entry", "data-id": backup.id, tabindex: "0" },
      h(
        "div",
        {},
        h("b", {}, backup.label),
        " ",
        h("span", { class: "chip" }, KIND_TEXT[backup.kind]),
      ),
      h(
        "div",
        { class: "actions" },
        restorable(backup)
          ? button("Restore", () => restoreAll(backup), "primary")
          : null,
        restorable(backup) ? button("Choose…", () => choose(backup)) : null,
        restorable(backup)
          ? button("Export .reg…", () => exportReg(backup))
          : null,
        button("Delete", () => remove(backup)),
      ),
      h("div", { class: "meta" }, facts.filter(Boolean).join(" · ")),
    );
  };

  const load = async () => {
    backups = await api.listBackups();
    listing.replaceChildren(
      ...(backups.length > 0
        ? backups.map(card)
        : [
            h(
              "p",
              { class: "empty" },
              "No backups yet. Cleaning creates one; so does Back up a key.",
            ),
          ]),
    );
  };

  byId("backup-key", HTMLButtonElement).addEventListener("click", () =>
    run(backUpKey),
  );
  const full = byId("backup-full", HTMLButtonElement);
  full.disabled = !context.info.elevated;
  full.title = context.info.elevated
    ? "Save the whole registry, to put back at a restart"
    : "Needs administrator rights";
  full.addEventListener("click", () => run(backUpFull));
  byId("backup-delete-all", HTMLButtonElement).addEventListener("click", () =>
    run(async () => {
      if (backups.length === 0) return;
      const yes = await confirm(
        "Delete all backups",
        `Delete all ${backups.length} backups? Nothing in them can be restored after this.`,
        "Delete all",
        "danger",
      );
      if (yes) await api.deleteAllBackups();
    }),
  );
  onMenu("backups", (target, at) => {
    const id = target.closest<HTMLElement>("[data-id]")?.dataset["id"];
    const backup = backups.find((b) => b.id === id);
    if (!backup) return;
    const usable = restorable(backup);
    run(async () => {
      const chosen = await popup(
        [
          item("restore", "&Restore", usable),
          item("choose", "&Choose what to restore…", usable),
          item("export", "E&xport as .reg…", usable),
          separator,
          item("delete", "&Delete"),
          item("deleteAll", "Delete &all…"),
        ],
        at,
      );
      if (chosen === "restore") await restoreAll(backup);
      if (chosen === "choose") await choose(backup);
      if (chosen === "export") await exportReg(backup);
      if (chosen === "delete") await remove(backup);
      if (chosen === "deleteAll")
        byId("backup-delete-all", HTMLButtonElement).click();
    });
  });

  /** Pick entries of one backup to put back. */
  async function choose(backup: Summary): Promise<void> {
    const items = await api.backupItems(backup.id);
    const picked = await pickItems(backup, items);
    if (picked && picked.length > 0) await restore(backup, picked);
  }

  return { load };
}

function pickItems(
  backup: Summary,
  items: ItemView[],
): Promise<number[] | null> {
  return new Promise((resolve) => {
    const modal = new Modal("Choose what to restore", true);
    const boxes = items.map((entry) => {
      const box = h("input", { type: "checkbox", disabled: !entry.restorable });
      box.dataset["index"] = String(entry.index);
      return h(
        "label",
        { class: "restore-item" },
        box,
        h("code", {}, entry.entry),
        h(
          "small",
          { class: "muted" },
          entry.restorable
            ? entry.detail
            : `${entry.state}${entry.reason ? `: ${entry.reason}` : ""}`,
        ),
      );
    });
    const done = (answer: number[] | null) => {
      modal.close();
      resolve(answer);
    };
    modal.dismissWith(() => done(null));
    modal.show(
      `Restore from "${backup.label}"`,
      [h("div", { class: "restore-list" }, ...boxes)],
      [
        { label: "Cancel", value: "no" },
        { label: "Restore ticked", value: "yes", kind: "primary" },
      ],
      (value) => {
        const ticked = boxes
          .map((label) => label.querySelector("input"))
          .filter((box): box is HTMLInputElement => box !== null && box.checked)
          .map((box) => Number(box.dataset["index"]));
        done(value === "yes" ? ticked : null);
      },
    );
  });
}

function backUpKey(): Promise<void> {
  return new Promise((resolve) => {
    const modal = new Modal("Back up a key");
    const key = h("input", {
      id: "backup-key-path",
      maxlength: "1000",
      placeholder: "HKEY_CURRENT_USER\\Software\\Example",
    });
    const label = h("input", {
      id: "backup-key-label",
      maxlength: "200",
      placeholder: "Optional",
    });
    const error = h("p", { class: "error", role: "alert" });
    const done = () => {
      modal.close();
      resolve();
    };
    modal.dismissWith(done);
    modal.show(
      "Back up a key",
      [
        h("label", { class: "field" }, "Key, with everything beneath it", key),
        h("label", { class: "field" }, "Name for this backup", label),
        error,
      ],
      [
        { label: "Cancel", value: "no" },
        {
          label: "Back up",
          value: "yes",
          kind: "primary",
          id: "backup-key-go",
        },
      ],
      (value) => {
        if (value !== "yes") return done();
        api
          .backUpKey(key.value, label.value)
          .then(done)
          .catch((failure: unknown) => {
            error.textContent = message(failure);
          });
      },
    );
    key.focus();
  });
}
