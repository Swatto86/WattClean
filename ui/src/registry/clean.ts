/** Removing the ticked findings of a scan or search, after asking. */
import { api, message, type CleanOutcome, type ResultList } from "./api.ts";
import type { AppContext } from "./context.ts";
import { inform, Modal } from "./dialog.ts";
import { h } from "./dom.ts";
import { count } from "./format.ts";
import type { FindingsTree } from "./results.ts";

interface Choices {
  restorePoint: boolean;
  force: boolean;
  fullBackup: boolean;
}

/** A clean is being asked for or running: a second click or a held Enter waits. */
let cleaning = false;

/** Ask, then remove; returns once the person has read the outcome. */
export async function cleanTicked(
  context: AppContext,
  list: ResultList,
  tree: FindingsTree,
): Promise<void> {
  const ids = tree.tickedIds();
  if (ids.length === 0 || cleaning) return;
  cleaning = true;
  try {
    await confirmAndClean(context, list, tree, ids);
  } finally {
    cleaning = false;
  }
}

async function confirmAndClean(
  context: AppContext,
  list: ResultList,
  tree: FindingsTree,
  ids: number[],
): Promise<void> {
  const choices = await confirmClean(context, ids.length);
  if (choices === null) return;
  try {
    const outcome = await api.clean(
      list,
      ids,
      choices.restorePoint,
      choices.force,
      choices.fullBackup,
    );
    tree.remove(outcome.removedIds);
    context.backupsChanged();
    await inform("Clean finished", describe(outcome));
  } catch (error: unknown) {
    await inform("Nothing was removed", message(error));
  }
}

/** The outcome in plain words. */
export function describe(outcome: CleanOutcome): string {
  const forced =
    outcome.forced > 0 ? ` (${outcome.forced} after taking ownership)` : "";
  const lines = [
    `${count(outcome.removed, "entry was", "entries were")} removed${forced}.`,
  ];
  if (outcome.backup) {
    lines.push(
      "Everything removed is in the Backup Center, where it can be restored.",
    );
  }
  if (outcome.fullBackupError) {
    lines.push(`No full registry backup was made: ${outcome.fullBackupError}`);
  }
  if (outcome.restorePointError) {
    lines.push(
      `No System Restore point was made: ${outcome.restorePointError}`,
    );
  }
  for (const [what, why] of outcome.warnings) {
    lines.push(`Removed, but: ${what} (${why})`);
  }
  for (const [what, why] of outcome.failed.slice(0, 10)) {
    lines.push(`Not removed: ${what} (${why})`);
  }
  if (outcome.failed.length > 10) {
    lines.push(`…and ${outcome.failed.length - 10} more not removed.`);
  }
  return lines.join("\n");
}

/**
 * The confirmation: resolves with the person's choices, or null when they
 * cancelled. Both choices need administrator rights.
 */
function confirmClean(
  context: AppContext,
  entries: number,
): Promise<Choices | null> {
  return new Promise((resolve) => {
    const modal = new Modal("Remove entries");
    const elevated = context.info.elevated;
    const point = h("input", {
      type: "checkbox",
      id: "clean-restore-point",
      disabled: !elevated,
    });
    point.checked = elevated && context.settings().restorePoint;
    const force = h("input", {
      type: "checkbox",
      id: "clean-force",
      disabled: !elevated,
    });
    force.checked = elevated && context.settings().forceDelete;
    const full = h("input", {
      type: "checkbox",
      id: "clean-full-backup",
      disabled: !elevated,
    });
    full.checked = elevated && context.settings().fullBackup;
    const done = (answer: Choices | null) => {
      modal.close();
      resolve(answer);
    };
    modal.dismissWith(() => done(null));
    modal.show(
      "Remove entries",
      [
        h(
          "p",
          {},
          `Remove ${count(entries, "entry", "entries")} from the registry? Each is backed up first and can be put back from the Backup Center.`,
        ),
        h(
          "label",
          { class: "check" },
          point,
          elevated
            ? "Create a System Restore point first"
            : "A System Restore point needs administrator rights",
        ),
        h(
          "label",
          { class: "check" },
          full,
          elevated
            ? "Make a full registry backup first (a few hundred MB)"
            : "A full registry backup needs administrator rights",
        ),
        h(
          "label",
          { class: "check" },
          force,
          elevated
            ? "Take ownership of protected keys to remove them (their owner and permissions are kept in the backup)"
            : "Removing protected keys needs administrator rights",
        ),
      ],
      [
        { label: "Cancel", value: "no" },
        { label: "Remove", value: "yes", kind: "danger", id: "clean-confirm" },
      ],
      (value) =>
        done(
          value === "yes"
            ? {
                restorePoint: point.checked,
                force: force.checked,
                fullBackup: full.checked,
              }
            : null,
        ),
    );
  });
}
