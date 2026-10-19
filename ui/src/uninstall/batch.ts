/**
 * Uninstalling several programs one at a time: one confirmation and one set
 * of options for them all, then each program's own wizard in list order
 * (uninstaller, leftover scan, removal), and a summary at the end.
 */
import { api, type Program, type SessionView } from "./api.ts";
import { Modal } from "./dialog.ts";
import { h } from "./dom.ts";
import { plural } from "./format.ts";
import {
  adminButton,
  batchForm,
  type ConfirmForm,
  message,
  type Row,
  stepList,
} from "./wizard-view.ts";
import { Wizard, type WizardContext } from "./wizard.ts";

/** One chosen program: its open uninstall, or why it could not open. */
type Entry =
  { name: string; session: SessionView } | { name: string; refused: string };

export async function uninstallMany(
  programs: readonly Program[],
  context: WizardContext,
): Promise<void> {
  const title = `Uninstall ${plural(programs.length, "program")}`;
  const modal = new Modal(title, true);
  let cancelled = false;
  const reading = h("p", { class: "muted", role: "status" }, "");
  modal.dismissWith(() => (cancelled = true));
  modal.show(
    `${title}?`,
    [reading],
    [{ label: "Cancel", value: "cancel", id: "batch-cancel" }],
    () => (cancelled = true),
  );
  // Every program is read before any uninstaller runs: one program's
  // uninstaller may delete files another is known by.
  const entries: Entry[] = [];
  for (const [index, program] of programs.entries()) {
    if (cancelled) break;
    reading.textContent = `Reading what each program is known by… (${index + 1} of ${programs.length})`;
    try {
      entries.push({
        name: program.name,
        session: await api.beginUninstall(program.id),
      });
    } catch (error: unknown) {
      entries.push({ name: program.name, refused: message(error) });
    }
  }
  const sessions = entries.flatMap((entry) =>
    "session" in entry ? [entry.session] : [],
  );
  const refused = entries.flatMap((entry): Array<[string, string]> =>
    "refused" in entry ? [[entry.name, entry.refused]] : [],
  );
  const close = () => {
    modal.close();
    context.onDone();
  };
  if (cancelled) {
    for (const session of sessions)
      await api.endUninstall(session.id).catch(() => undefined);
    close();
    return;
  }
  if (sessions.length === 0) {
    modal.dismissWith(close);
    modal.show(
      "Nothing to uninstall",
      [
        stepList(
          refused.map(([name, reason]): Row => [name, "failed", reason]),
          "batch-summary",
        ),
      ],
      [{ label: "Close", value: "close", kind: "primary", id: "batch-close" }],
      close,
    );
    return;
  }
  const form = batchForm(
    sessions,
    refused,
    context.settings,
    adminButton(context.restartAsAdmin),
  );
  if (!(await confirmed(modal, sessions.length, form))) {
    for (const session of sessions)
      await api.endUninstall(session.id).catch(() => undefined);
    close();
    return;
  }

  const choice = form.choice();
  const quiet = form.quiet();
  // One restore point, before the first program, covers the batch.
  let restorePoint = form.restorePoint();
  let stopped = false;
  const rows: Row[] = [];
  for (const entry of entries) {
    if ("refused" in entry) {
      rows.push([entry.name, "failed", `Left out: ${entry.refused}`]);
      continue;
    }
    if (stopped) {
      await api.endUninstall(entry.session.id).catch(() => undefined);
      rows.push([entry.name, "todo", "Not started: the batch was stopped."]);
      continue;
    }
    const wizard = new Wizard(entry.session, context, {
      modal,
      index: sessions.indexOf(entry.session),
      total: sessions.length,
    });
    wizard
      .run(choice, restorePoint, quiet)
      .catch((error: unknown) => wizard.fail(error));
    restorePoint = false;
    const outcome = await wizard.ended;
    rows.push([entry.name, outcome.ok ? "done" : "failed", outcome.summary]);
    stopped = outcome.stop;
  }

  modal.dismissWith(close);
  modal.show(
    `${title}: done`,
    [stepList(rows, "batch-summary")],
    [{ label: "Close", value: "close", kind: "primary", id: "batch-close" }],
    close,
  );
}

/** The batch's first step; true when the person starts it. */
function confirmed(
  modal: Modal,
  count: number,
  form: ConfirmForm,
): Promise<boolean> {
  return new Promise((resolve) => {
    modal.dismissWith(() => resolve(false));
    modal.show(
      `Uninstall ${plural(count, "program")}?`,
      form.nodes,
      [
        { label: "Cancel", value: "cancel" },
        {
          label: `Uninstall ${plural(count, "program")}`,
          value: "go",
          kind: "primary",
          id: "wizard-start",
        },
      ],
      (value) => resolve(value === "go"),
    );
  });
}
