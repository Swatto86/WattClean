/** What each wizard step shows; the flow itself is in wizard.ts. */
import type { Mode, Removal, SessionView, Settings } from "./api.ts";
import { h } from "./dom.ts";
import { bytes, countsText, plural } from "./format.ts";

export type Choice = Mode | "builtin";
export type State = "busy" | "done" | "failed" | "todo";
export type Row = [title: string, state: State, note: string];

export const message = (error: unknown) =>
  error instanceof Error ? error.message : String(error);

const MODE_TEXT: Array<[Choice, string, string]> = [
  [
    "builtin",
    "Uninstaller only",
    "Run the program's own uninstaller and stop there.",
  ],
  [
    "safe",
    "Safe",
    "Also remove the folders and keys named after the program. Fastest.",
  ],
  [
    "moderate",
    "Moderate",
    "Also follow its files through Windows' registrations. Recommended.",
  ],
  [
    "advanced",
    "Advanced",
    "Also search the whole software registry and data folders. Slowest.",
  ],
];

export interface ConfirmForm {
  nodes: HTMLElement[];
  choice: () => Choice;
  restorePoint: () => boolean;
  quiet: () => boolean;
}

/** The first step: what is being removed, the scan mode and options. */
export function confirmForm(
  session: SessionView,
  settings: Settings,
  adminButton: HTMLButtonElement,
): ConfirmForm {
  const modes = h("div", {
    class: "modes",
    role: "radiogroup",
    "aria-label": "Leftover scan",
  });
  for (const [value, title, text] of MODE_TEXT) {
    const input = h("input", {
      type: "radio",
      name: "mode",
      value,
      id: `mode-${value}`,
    });
    input.checked = value === settings.mode;
    modes.append(
      h(
        "label",
        { class: "mode" },
        input,
        h("b", {}, title),
        h("small", {}, text),
      ),
    );
  }
  const restore = h("input", { type: "checkbox", id: "wizard-restore" });
  restore.checked = settings.restorePoint;
  const quiet = h("input", { type: "checkbox", id: "wizard-quiet" });
  quiet.checked = settings.quiet;
  const facts = [session.publisher, session.version].filter(Boolean);
  const nodes = [
    h("p", {}, facts.join(" · ") || "Unknown publisher"),
    session.installDirs.length > 0
      ? h(
          "p",
          { class: "muted" },
          `Installed in ${session.installDirs.join(", ")}`,
        )
      : null,
    session.uninstallCommand
      ? h("p", { class: "muted" }, h("code", {}, session.uninstallCommand))
      : null,
    session.needsAdmin
      ? h(
          "div",
          { class: "notice" },
          "This program is installed for all users, so removing it needs administrator rights. ",
          adminButton,
        )
      : null,
    session.loggedItems !== null
      ? h(
          "p",
          { class: "notice", id: "wizard-logged" },
          `Installed with tracing: the ${session.loggedItems} items its install log recorded are checked first, whichever mode you choose.`,
        )
      : null,
    h("h3", {}, "After the uninstaller"),
    modes,
    h(
      "label",
      { class: "check" },
      restore,
      "Create a System Restore point first",
    ),
    h(
      "label",
      { class: "check" },
      quiet,
      "Run the uninstaller silently if it allows it",
    ),
  ].filter((node) => node !== null);
  return {
    nodes,
    choice: () => {
      const picked =
        modes.querySelector<HTMLInputElement>("input:checked")?.value;
      return MODE_TEXT.find(([mode]) => mode === picked)?.[0] ?? "moderate";
    },
    restorePoint: () => restore.checked,
    quiet: () => quiet.checked,
  };
}

/** A checklist of steps with a mark for each one's state. */
export function stepList(rows: Row[]): HTMLElement {
  const marks: Record<State, string> = {
    busy: "",
    done: "✓",
    failed: "!",
    todo: "",
  };
  return h(
    "ol",
    { class: "steps", id: "wizard-steps" },
    ...rows.map(([title, state, note]) =>
      h(
        "li",
        { "data-state": state },
        h(
          "span",
          { class: `mark ${state}`, "aria-hidden": "true" },
          marks[state],
        ),
        h(
          "div",
          {},
          h("b", {}, title),
          note ? h("p", { class: "muted" }, note) : null,
        ),
      ),
    ),
  );
}

/** The result of a removal: counts, space freed, and anything not removed. */
export function removalSummary(removal: Removal): HTMLElement[] {
  const stats = h(
    "div",
    { class: "summary-grid" },
    ...[
      ["Registry keys", removal.removed.keys],
      ["Registry values", removal.removed.values],
      ["Folders", removal.removed.folders],
      ["Files", removal.removed.files],
    ].map(([label, value]) =>
      h("div", { class: "stat" }, h("b", {}, String(value)), String(label)),
    ),
  );
  const nodes: HTMLElement[] = [
    h(
      "p",
      { id: "removal-summary" },
      `Removed ${countsText(removal.removed)}, freeing ${bytes(removal.bytes)}.`,
    ),
    stats,
  ];
  if (removal.atRestart > 0) {
    nodes.push(
      h(
        "p",
        { class: "notice" },
        `${plural(removal.atRestart, "item")} in use will be deleted when Windows restarts.`,
      ),
    );
  }
  for (const failure of removal.failed) {
    nodes.push(
      h("p", { class: "notice bad" }, `${failure.item}: ${failure.error}`),
    );
  }
  return nodes;
}
