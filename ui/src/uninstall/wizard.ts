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
import { type Action, confirm, inform, Modal } from "./dialog.ts";
import { h } from "./dom.ts";
import { plural } from "./format.ts";
import { LeftoverList } from "./leftovers.ts";
import {
  adminButton,
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

/** A program's place in a batch, whose dialog it shares. */
export interface Place {
  modal: Modal;
  index: number;
  total: number;
}

/** How one program's uninstall ended, for a batch's summary. */
export interface Outcome {
  summary: string;
  ok: boolean;
  /** The person chose to stop the batch here. */
  stop: boolean;
}

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

export class Wizard {
  private readonly modal: Modal;
  private closed = false;
  /** Set once the uninstaller is asked for, so a second click is ignored. */
  private started = false;
  private outcome: Outcome = {
    summary: "Closed before it finished.",
    ok: false,
    stop: false,
  };
  private settle: (outcome: Outcome) => void = () => undefined;
  /** Settles once this uninstall is closed. */
  readonly ended = new Promise<Outcome>((resolve) => {
    this.settle = resolve;
  });

  constructor(
    private readonly session: SessionView,
    private readonly context: WizardContext,
    private readonly place: Place | null = null,
  ) {
    this.modal = place?.modal ?? new Modal(`Uninstall ${session.name}`, true);
  }

  /** A step's title, with the program's place when it is part of a batch. */
  private title(text: string): string {
    return this.place
      ? `${text} (${this.place.index + 1} of ${this.place.total})`
      : text;
  }

  /** A batch keeps its dialog and refreshes the list once, at its end. */
  private async end(): Promise<void> {
    if (this.closed) return;
    this.closed = true;
    if (!this.place) this.modal.close();
    try {
      await api.endUninstall(this.session.id);
    } catch (error: unknown) {
      // The uninstall is done; only its history entry was not written.
      const note = `Its history entry could not be saved: ${message(error)}`;
      this.outcome = {
        ...this.outcome,
        summary: `${this.outcome.summary} ${note}`,
      };
      if (!this.place) void inform("Uninstall history", note);
    } finally {
      if (!this.place) this.context.onDone();
      this.settle(this.outcome);
    }
  }

  /** Ends this program's steps with an error nothing else caught. */
  fail(error: unknown): void {
    if (!this.closed)
      this.finishStep(`Something went wrong: ${message(error)}`, false);
  }

  confirmStep(): void {
    const form = confirmForm(
      this.session,
      this.context.settings,
      adminButton(this.context.restartAsAdmin),
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
          this.run(form.choice(), form.restorePoint(), form.quiet()).catch(
            (error: unknown) => this.fail(error),
          );
      },
    );
  }

  /** Runs the program's uninstaller, then the chosen leftover scan. */
  async run(
    choice: Choice,
    restorePoint: boolean,
    quiet: boolean,
  ): Promise<void> {
    if (this.started) return;
    this.started = true;
    this.modal.dismissWith(null);
    let restoreRow: Row | null = null;
    let carryOn = false;
    const render = (state: State, note: string, actions: Action[]) => {
      const rows: Row[] = restoreRow ? [restoreRow] : [];
      rows.push(["Program's own uninstaller", state, note]);
      this.modal.show(
        this.title(`Uninstalling ${this.session.name}`),
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
        await api.createRestorePoint(this.session.id, this.place?.total ?? 1);
        restoreRow = ["Restore point", "done", "Created."];
      } catch (error: unknown) {
        restoreRow = ["Restore point", "failed", message(error)];
      }
    }
    let started = true;
    try {
      render("busy", "Starting the uninstaller…", []);
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
      this.outcome = { summary: message(error), ok: false, stop: false };
      const actions: Action[] = [
        { label: this.place ? "Skip this program" : "Close", value: "close" },
      ];
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
      this.title(`Scanning for leftovers of ${this.session.name}`),
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
      this.finishStep(`The scan did not finish: ${message(error)}`, false);
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
      this.title(
        `Found ${plural(report.items.length, "leftover")} of ${this.session.name}`,
      ),
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
      this.title(`Removing leftovers of ${this.session.name}`),
      [stepList([["Backing up and removing", "busy", ""]])],
      [],
      () => undefined,
    );
    try {
      const removal = await api.removeLeftovers(this.session.id, ids);
      const nodes = removalSummary(removal);
      this.finishNodes(nodes, nodes[0]?.textContent ?? "", true);
    } catch (error: unknown) {
      this.finishStep(`Nothing more was removed: ${message(error)}`, false);
    }
  }

  private finishStep(text: string, ok = true): void {
    this.finishNodes([h("p", { id: "wizard-result" }, text)], text, ok);
  }

  /**
   * The last step. In a batch it leads on to the next program or stops the
   * batch; Esc carries on, as Close does alone.
   */
  private finishNodes(
    nodes: HTMLElement[],
    summary: string,
    ok: boolean,
  ): void {
    this.outcome = { summary, ok, stop: false };
    const last = !this.place || this.place.index + 1 === this.place.total;
    const actions: Action[] = last
      ? [
          {
            label: "Close",
            value: "close",
            kind: "primary",
            id: "wizard-close",
          },
        ]
      : [
          { label: "Stop here", value: "stop", id: "wizard-stop" },
          {
            label: "Next program",
            value: "next",
            kind: "primary",
            id: "wizard-next",
          },
        ];
    // Esc closes; in a batch it stops before the next program.
    this.modal.dismissWith(() => {
      if (!last) this.outcome.stop = true;
      void this.end();
    });
    this.modal.show(
      this.title(`${this.session.name}: done`),
      nodes,
      actions,
      (value) => {
        if (value === "stop") this.outcome.stop = true;
        void this.end();
      },
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
