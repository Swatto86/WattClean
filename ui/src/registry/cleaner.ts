/** The Registry Cleaner view: choose a mode, scan, review, clean. */
import {
  api,
  message,
  MODES,
  type Category,
  type Mode,
  type Report,
} from "./api.ts";
import { cleanTicked } from "./clean.ts";
import { exclude, type AppContext } from "./context.ts";
import { byId, h, PREFIX } from "./dom.ts";
import { count } from "./format.ts";
import { FindingsTree } from "./results.ts";

/** Follow a running scan's phase until `done` settles. */
export function follow(
  phase: HTMLElement,
  done: Promise<unknown>,
): Promise<void> {
  let running = true;
  const tick = async () => {
    while (running) {
      phase.textContent = await api.scanProgress().catch(() => "");
      await new Promise((resolve) => setTimeout(resolve, 250));
    }
  };
  void tick();
  return done.then(
    () => {
      running = false;
    },
    () => {
      running = false;
    },
  );
}

/** Notes under the summary: accounts not read and the like. */
export function showNotes(target: HTMLElement, report: Report): void {
  const notes = report.notes.map((note) => h("p", { class: "notice" }, note));
  if (report.cancelled) {
    notes.unshift(
      h("p", { class: "notice" }, "Stopped early: the results are partial."),
    );
  }
  target.replaceChildren(...notes);
}

export function wireCleaner(context: AppContext): void {
  const form = byId("cleaner-start", HTMLFormElement);
  const custom = byId("cleaner-custom", HTMLFieldSetElement);
  const all = byId("cleaner-all", HTMLInputElement);
  const error = byId("cleaner-error", HTMLParagraphElement);
  const busy = byId("cleaner-busy", HTMLDivElement);
  const results = byId("cleaner-results", HTMLDivElement);
  const toolbar = byId("cleaner-toolbar", HTMLDivElement);
  const cleanButton = byId("cleaner-clean", HTMLButtonElement);
  const summary = byId("cleaner-summary", HTMLParagraphElement);

  const boxes = new Map<Category, HTMLInputElement>();
  for (const info of context.titles.values()) {
    const box = h("input", { type: "checkbox", value: info.id });
    box.checked = context.settings().custom.includes(info.id);
    boxes.set(info.id, box);
    custom.append(
      h("label", { class: "check", title: info.description }, box, info.title),
    );
  }
  const radios = [
    ...form.querySelectorAll<HTMLInputElement>("input[name=mode]"),
  ];
  const chosenMode = (): Mode =>
    MODES.find((mode) => radios.some((r) => r.checked && r.value === mode)) ??
    "safe";
  for (const radio of radios) {
    radio.checked = radio.value === context.settings().mode;
    radio.addEventListener("change", () => {
      custom.hidden = chosenMode() !== "custom";
    });
  }
  custom.hidden = chosenMode() !== "custom";
  all.checked = context.settings().allAccounts;

  const tree = new FindingsTree({
    root: byId("cleaner-tree", HTMLDivElement),
    list: "cleaner",
    titles: context.titles,
    elevated: context.info.elevated,
    exclude: (key) => exclude(context, key),
    changed: () => {
      const ticked = tree.tickedIds().length;
      cleanButton.textContent = `Clean (${ticked})`;
      cleanButton.disabled = ticked === 0;
      const locked = tree.lockedCount();
      summary.textContent =
        tree.count() === 0
          ? "The registry is clean: nothing was found."
          : `${count(tree.count(), "entry", "entries")} found, ${ticked} ticked.` +
            (locked > 0
              ? ` ${count(locked, "entry needs", "entries need")} administrator rights.`
              : "");
    },
  });

  const showStart = () => {
    form.hidden = false;
    results.hidden = true;
    toolbar.hidden = true;
  };

  /** Scan as the form says; false when it failed (the form says why). */
  const scan = async (mode: Mode, categories: Category[]): Promise<boolean> => {
    try {
      await context.update((current) => ({
        ...current,
        mode,
        custom: mode === "custom" ? categories : current.custom,
        allAccounts: all.checked,
      }));
      form.hidden = true;
      busy.hidden = false;
      const running = api.scanRegistry(mode, categories, all.checked);
      await follow(byId("cleaner-phase", HTMLParagraphElement), running);
      const report = await running;
      showNotes(byId("cleaner-notes", HTMLDivElement), report);
      tree.show(report.findings);
      results.hidden = false;
      toolbar.hidden = false;
      return true;
    } catch (failure: unknown) {
      error.textContent = message(failure);
      showStart();
      return false;
    } finally {
      busy.hidden = true;
    }
  };

  form.addEventListener("submit", (event) => {
    event.preventDefault();
    error.textContent = "";
    // Revo's Quick Clean: the same scan, then straight to the clean dialog.
    const quick = event.submitter?.id === `${PREFIX}cleaner-quick`;
    const mode = chosenMode();
    const categories = [...boxes]
      .filter(([, box]) => box.checked)
      .map(([id]) => id);
    if (mode === "custom" && categories.length === 0) {
      error.textContent = "Tick at least one category.";
      return;
    }
    void (async () => {
      if ((await scan(mode, categories)) && quick) {
        await cleanTicked(context, "cleaner", tree);
      }
    })();
  });
  byId("cleaner-cancel", HTMLButtonElement).addEventListener("click", () => {
    void api.cancelScan();
  });
  byId("cleaner-again", HTMLButtonElement).addEventListener("click", showStart);
  cleanButton.addEventListener(
    "click",
    () => void cleanTicked(context, "cleaner", tree),
  );
}
