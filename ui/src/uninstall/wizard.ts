/**
 * The uninstall wizard: confirm and choose a scan mode, make a restore point,
 * run the program's own uninstaller and wait for it, scan, then remove the
 * leftovers the person ticks. A forced uninstall starts at the scan.
 */
import {
  api,
  type Mode,
  type Program,
  type SessionView,
  type Settings,
} from "./api.ts";
import { type Action, confirm, Modal } from "./dialog.ts";
import { h } from "./dom.ts";
import { plural } from "./format.ts";
import { LeftoverList } from "./leftovers.ts";
import {
  type Choice,
  confirmForm,
  message,
  removalSummary,
  type Row,
  type State,
  stepList,
} from "./wizard-view.ts";

export interface WizardContext {
  settings: Settings;
  restartAsAdmin: () => void;
  onDone: () => void;
}

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

class Wizard {
  private readonly modal: Modal;
  private closed = false;

  constructor(
    private readonly session: SessionView,
    private readonly context: WizardContext,
  ) {
    this.modal = new Modal(`Uninstall ${session.name}`, true);
  }

  private async end(): Promise<void> {
    if (this.closed) return;
    this.closed = true;
    this.modal.close();
    try {
      await api.endUninstall(this.session.id);
    } finally {
      this.context.onDone();
    }
  }

  confirmStep(): void {
    const form = confirmForm(
      this.session,
      this.context.settings,
      this.adminButton(),
    );
    this.modal.dismissWith(() => void this.end());
    this.modal.show(
      `Uninstall ${this.session.name}?`,
      form.nodes,
      [
        { label: "Cancel", value: "cancel" },
        {
          label: "Uninstall",
          value: "go",
          kind: "primary",
          id: "wizard-start",
        },
      ],
      (value) => {
        if (value === "cancel") void this.end();
        else
          void this.runStep(form.choice(), form.restorePoint(), form.quiet());
      },
    );
  }

  private adminButton(): HTMLButtonElement {
    const button = h(
      "button",
      { class: "ghost", type: "button", id: "wizard-admin" },
      "Restart as administrator",
    );
    button.addEventListener("click", () => this.context.restartAsAdmin());
    return button;
  }

  private async runStep(
    choice: Choice,
    restorePoint: boolean,
    quiet: boolean,
  ): Promise<void> {
    this.modal.dismissWith(null);
    let restoreRow: Row | null = null;
    let carryOn = false;
    const render = (state: State, note: string, actions: Action[]) => {
      const rows: Row[] = restoreRow ? [restoreRow] : [];
      rows.push(["Program's own uninstaller", state, note]);
      this.modal.show(
        `Uninstalling ${this.session.name}`,
        [stepList(rows)],
        actions,
        (value) => {
          if (value === "continue") carryOn = true;
          if (value === "close") void this.end();
        },
      );
    };
    if (restorePoint) {
      restoreRow = [
        "Restore point",
        "busy",
        "Asking Windows for a restore point…",
      ];
      render("todo", "", []);
      try {
        await api.createRestorePoint(this.session.id);
        restoreRow = ["Restore point", "done", "Created."];
      } catch (error: unknown) {
        restoreRow = ["Restore point", "failed", message(error)];
      }
    }
    let started = true;
    try {
      const command = await api.runUninstaller(this.session.id, quiet);
      render(
        "busy",
        `Running ${command}. Finish its steps; this continues on its own when it closes.`,
        [
          {
            label: "It has finished — continue",
            value: "continue",
            id: "wizard-continue",
          },
        ],
      );
    } catch (error: unknown) {
      started = false;
      const actions: Action[] = [{ label: "Close", value: "close" }];
      if (choice !== "builtin") {
        actions.push({
          label: "Scan for leftovers anyway",
          value: "continue",
          kind: "primary",
          id: "wizard-scan-anyway",
        });
      }
      render("failed", message(error), actions);
    }
    while (!this.closed && !carryOn) {
      await sleep(started ? 500 : 200);
      if (!started) continue;
      const finished = await api
        .uninstallerFinished(this.session.id)
        .catch(() => true);
      if (finished) break;
    }
    if (!this.closed) await this.afterUninstaller(choice);
  }

  private async afterUninstaller(choice: Choice): Promise<void> {
    if (choice === "builtin") {
      this.finishStep(
        "The program's uninstaller has finished. No leftover scan was run.",
      );
      return;
    }
    await this.scanStep(choice);
  }

  async scanStep(mode: Mode): Promise<void> {
    this.modal.dismissWith(null);
    const area = h(
      "p",
      { class: "muted", id: "scan-area", role: "status" },
      "Starting…",
    );
    this.modal.show(
      `Scanning for leftovers of ${this.session.name}`,
      [
        stepList([
          [
            "Scanning",
            "busy",
            `${mode[0]?.toUpperCase()}${mode.slice(1)} scan`,
          ],
        ]),
        area,
      ],
      [{ label: "Cancel scan", value: "cancel", id: "wizard-cancel-scan" }],
      () => void api.cancelScan(this.session.id),
    );
    let scanning = true;
    void (async () => {
      while (scanning && !this.closed) {
        await sleep(300);
        const text = await api.scanProgress(this.session.id).catch(() => "");
        if (scanning && text) area.textContent = text;
      }
    })();
    try {
      const report = await api.scanLeftovers(this.session.id, mode);
      scanning = false;
      this.leftoverStep(report);
    } catch (error: unknown) {
      scanning = false;
      this.finishStep(`The scan did not finish: ${message(error)}`);
    }
  }

  private leftoverStep(
    report: Awaited<ReturnType<typeof api.scanLeftovers>>,
  ): void {
    if (report.items.length === 0) {
      this.finishStep("Nothing was left behind.");
      return;
    }
    const list = new LeftoverList(report, () => {
      const remove = document.getElementById("un-wizard-remove");
      if (remove instanceof HTMLButtonElement)
        remove.disabled = list.selectedIds().length === 0;
    });
    this.modal.show(
      `Found ${plural(report.items.length, "leftover")} of ${this.session.name}`,
      [
        h(
          "p",
          { class: "muted" },
          "Check the list. Everything removed is backed up first and can be restored from Backups.",
        ),
        list.element,
      ],
      [
        { label: "Skip", value: "skip", id: "wizard-skip" },
        {
          label: "Remove selected",
          value: "remove",
          kind: "danger",
          id: "wizard-remove",
        },
      ],
      (value) => {
        if (value === "skip") this.finishStep("Leftovers were kept.");
        else void this.remove(list.selectedIds());
      },
    );
    const remove = document.getElementById("un-wizard-remove");
    if (remove instanceof HTMLButtonElement)
      remove.disabled = list.selectedIds().length === 0;
  }

  private async remove(ids: number[]): Promise<void> {
    const sure = await confirm(
      "Remove leftovers?",
      `Remove ${plural(ids.length, "item")}? A backup is made first.`,
      "Remove",
      "danger",
    );
    if (!sure || this.closed) return;
    this.modal.show(
      `Removing leftovers of ${this.session.name}`,
      [stepList([["Backing up and removing", "busy", ""]])],
      [],
      () => undefined,
    );
    try {
      const removal = await api.removeLeftovers(this.session.id, ids);
      this.finishNodes(removalSummary(removal));
    } catch (error: unknown) {
      this.finishStep(`Nothing more was removed: ${message(error)}`);
    }
  }

  private finishStep(message: string): void {
    this.finishNodes([h("p", { id: "wizard-result" }, message)]);
  }

  private finishNodes(nodes: HTMLElement[]): void {
    this.modal.dismissWith(() => void this.end());
    this.modal.show(
      `${this.session.name}: done`,
      nodes,
      [{ label: "Close", value: "close", kind: "primary", id: "wizard-close" }],
      () => void this.end(),
    );
  }
}

export async function uninstall(
  program: Program,
  context: WizardContext,
): Promise<void> {
  const session = await api.beginUninstall(program.id);
  new Wizard(session, context).confirmStep();
}

export async function forced(
  name: string,
  folder: string,
  mode: Mode,
  context: WizardContext,
): Promise<void> {
  const session = await api.beginForced(name, folder);
  await new Wizard(session, context).scanStep(mode);
}
