/** The Backups and History views. */
import { api, type BackupSummary, type HistoryEntry } from "./api.ts";
import { confirm, inform } from "./dialog.ts";
import { byId, clear, h } from "./dom.ts";
import { bytes, countsText, plural, when } from "./format.ts";

const message = (error: unknown) =>
  error instanceof Error ? error.message : String(error);

function empty(text: string): HTMLElement {
  return h("p", { class: "empty" }, text);
}

export async function showBackups(): Promise<void> {
  const list = byId("backup-list", HTMLDivElement);
  let backups: BackupSummary[];
  try {
    backups = await api.listBackups();
  } catch (error: unknown) {
    list.replaceChildren(
      empty(`Could not read the backups: ${message(error)}`),
    );
    return;
  }
  clear(list);
  if (backups.length === 0)
    list.append(
      empty("No backups yet. They appear here after leftovers are removed."),
    );
  for (const backup of backups) list.append(backupCard(backup));
}

function backupCard(backup: BackupSummary): HTMLElement {
  const restore = h(
    "button",
    { class: "primary", type: "button", "data-action": "restore" },
    "Restore",
  );
  restore.disabled = !backup.restorable;
  restore.addEventListener("click", () => void restoreBackup(backup));
  const remove = h(
    "button",
    { class: "ghost", type: "button", "data-action": "delete" },
    "Delete",
  );
  remove.addEventListener("click", () => void deleteBackup(backup));
  const status =
    backup.restored !== null
      ? h("span", { class: "pill ok" }, `Restored ${when(backup.restored)}`)
      : h("span", { class: "pill warn" }, "Can be restored");
  return h(
    "article",
    { class: "card entry", "data-backup": backup.id },
    h("div", {}, h("b", {}, backup.program), " ", status),
    h("div", { class: "actions" }, restore, remove),
    h(
      "div",
      { class: "meta" },
      `${when(backup.created)} · ${plural(backup.items, "item")} · ${bytes(backup.bytes)}`,
    ),
  );
}

async function restoreBackup(backup: BackupSummary): Promise<void> {
  const sure = await confirm(
    "Restore backup?",
    `Put back the ${plural(backup.items, "item")} removed from ${backup.program}?`,
    "Restore",
  );
  if (!sure) return;
  try {
    const result = await api.restoreBackup(backup.id);
    const problems = result.failed
      .map((failure) => `${failure.item}: ${failure.error}`)
      .join("\n");
    await inform(
      "Restore finished",
      `Restored ${plural(result.restored, "item")}.${problems ? `\n\nNot restored:\n${problems}` : ""}`,
    );
  } catch (error: unknown) {
    await inform("Restore failed", message(error));
  }
  await showBackups();
}

async function deleteBackup(backup: BackupSummary): Promise<void> {
  const sure = await confirm(
    "Delete backup?",
    `Delete the backup of ${backup.program}? It can no longer be restored.`,
    "Delete",
    "danger",
  );
  if (!sure) return;
  try {
    await api.deleteBackup(backup.id);
  } catch (error: unknown) {
    await inform("Delete failed", message(error));
  }
  await showBackups();
}

const OUTCOMES: Record<HistoryEntry["uninstaller"]["state"], string> = {
  finished: "Uninstaller ran",
  failed: "Uninstaller failed",
  notRun: "Forced",
};

export async function showHistory(): Promise<void> {
  const list = byId("history-list", HTMLDivElement);
  let entries: HistoryEntry[];
  try {
    entries = await api.listHistory();
  } catch (error: unknown) {
    list.replaceChildren(
      empty(`Could not read the history: ${message(error)}`),
    );
    return;
  }
  clear(list);
  if (entries.length === 0) list.append(empty("Nothing uninstalled yet."));
  for (const entry of entries) {
    const mode = entry.mode ? `${entry.mode} scan` : "no scan";
    const removed = `Removed ${countsText(entry.removed)}${entry.bytes > 0 ? ` (${bytes(entry.bytes)})` : ""}`;
    list.append(
      h(
        "article",
        { class: "card entry", "data-history": entry.id },
        h(
          "div",
          {},
          h("b", {}, entry.program),
          entry.version ? ` ${entry.version}` : "",
        ),
        h(
          "div",
          { class: "actions" },
          h(
            "span",
            {
              class:
                entry.uninstaller.state === "failed" ? "pill bad" : "pill ok",
            },
            OUTCOMES[entry.uninstaller.state],
          ),
        ),
        h(
          "div",
          { class: "meta" },
          `${when(entry.time)} · ${mode} · ${removed}`,
        ),
      ),
    );
  }
}

export function wireHistory(): void {
  byId("clear-history", HTMLButtonElement).addEventListener(
    "click",
    async () => {
      if (
        !(await confirm(
          "Clear history?",
          "Forget every recorded uninstall? Backups are kept.",
          "Clear",
          "danger",
        ))
      )
        return;
      try {
        await api.clearHistory();
      } catch (error: unknown) {
        await inform("Could not clear the history", message(error));
      }
      await showHistory();
    },
  );
}
