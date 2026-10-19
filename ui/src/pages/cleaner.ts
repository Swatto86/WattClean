// The Cleaner page: tick cleaners and options on the left, preview or clean
// on the right. Ticks are saved as they change.

import { api, type Catalog, type Mode, type Report } from "../api.ts";
import type { Context, Page } from "../context.ts";
import { confirm } from "../dialog.ts";
import { commandButton, el, glyph, notice } from "../dom.ts";
import { plural } from "../format.ts";
import { trashName } from "../platform.ts";
import {
  defaults,
  everything,
  key,
  runnable,
  toggleCleaner,
} from "../selection.ts";
import { ProgressCard, renderReport } from "./results.ts";
import { Tree } from "./tree.ts";

export const cleanerPage = (ctx: Context): Page => {
  let catalog: Catalog | null = null;
  let ticked = new Set<string>();
  let query = "";
  let running = false;
  let saveTimer: number | undefined;

  const preview = commandButton(
    "cleaner-preview",
    "Preview",
    "preview",
    "secondary",
  );
  const clean = commandButton("cleaner-clean", "Clean", "clean", "primary");
  const stop = commandButton("cleaner-stop", "Stop", "stop", "secondary");
  stop.hidden = true;

  const results = el("div", { class: "scroll results", id: "cleaner-results" });
  const shownCount = el("span", { id: "tree-count" });
  const showAll = el("input", { type: "checkbox", id: "show-all" });
  const selectAll = el("button", {
    type: "button",
    class: "link",
    id: "select-all",
    text: "All",
    title: "Tick every option (a full clean)",
  });
  const selectDefaults = el("button", {
    type: "button",
    class: "link",
    id: "select-defaults",
    text: "Defaults",
  });
  const selectNone = el("button", {
    type: "button",
    class: "link",
    id: "select-none",
    text: "None",
  });

  const tree = new Tree({
    option: async (cleaner, optionId, wanted) => {
      const k = key(cleaner.id, optionId);
      const option = cleaner.options.find((o) => o.id === optionId);
      if (wanted && option?.warning) {
        const answer = await confirm({
          title: `${cleaner.name} › ${option.name}`,
          message: option.warning,
          ok: "Tick it",
        });
        if (!answer) return false;
      }
      if (wanted) ticked.add(k);
      else ticked.delete(k);
      changed();
      return true;
    },
    cleaner: (cleaner) => {
      ticked = toggleCleaner(cleaner, ticked);
      changed();
    },
  });

  const selected = (): string[] =>
    catalog
      ? runnable(catalog.cleaners, ticked, ctx.settings().hideIrrelevant)
      : [];

  const updateStatus = () => {
    const n = selected().length;
    ctx.setSelectionStatus(`${plural(n, "option")} ticked`);
    preview.disabled = running || n === 0;
    clean.disabled = running || n === 0;
  };

  const changed = () => {
    tree.sync(ticked);
    updateStatus();
    window.clearTimeout(saveTimer);
    saveTimer = window.setTimeout(() => {
      ctx
        .update({ selection: [...ticked] })
        .catch((e: unknown) => notice(String(e), true));
    }, 400);
  };

  const redraw = () => {
    if (!catalog) return;
    const hide = ctx.settings().hideIrrelevant;
    const shown = tree.render(catalog.cleaners, ticked, hide, query);
    shownCount.textContent = `${shown} of ${catalog.cleaners.length} cleaners shown`;
    showAll.checked = !hide;
    updateStatus();
  };

  const idle = () => {
    const n = selected().length;
    results.replaceChildren(
      el(
        "div",
        { class: "card hero", id: "cleaner-idle" },
        glyph("cleaner"),
        el("h2", { text: "Free space and clear your tracks" }),
        el("p", {
          text: n
            ? `${plural(n, "option")} ticked. Preview lists what would be removed without touching anything; Clean removes it.`
            : "Tick the cleaners and options you want on the left, then Preview to see what would go.",
        }),
      ),
    );
  };

  const load = async () => {
    try {
      catalog = await api.getCatalog();
      ticked = new Set(catalog.selection);
      redraw();
      if (!running && !results.querySelector("#report-tiles")) idle();
    } catch (error) {
      notice(`The cleaner list could not be read: ${String(error)}`, true);
    }
  };

  /** Ticks every option shown, after one look at the warnings not yet read. */
  const tickAll = async () => {
    if (!catalog) return;
    const all = everything(catalog.cleaners, ctx.settings().hideIrrelevant);
    const shown = new Set(all);
    const warnings: string[] = [];
    for (const cleaner of catalog.cleaners) {
      for (const option of cleaner.options) {
        const k = key(cleaner.id, option.id);
        if (option.warning && shown.has(k) && !ticked.has(k)) {
          warnings.push(`${cleaner.name} › ${option.name}: ${option.warning}`);
        }
      }
    }
    if (warnings.length) {
      const answer = await confirm({
        title: "Tick every option?",
        message: `All ${plural(all.length, "option")} will be ticked, including these, which each carry a warning.`,
        ok: "Tick all",
        list: warnings,
      });
      if (!answer) return;
    }
    ticked = new Set([...ticked, ...all]);
    changed();
  };

  const confirmClean = async (keys: string[]): Promise<boolean> => {
    const settings = ctx.settings();
    if (!settings.confirm || !catalog) return true;
    const lines: string[] = [];
    for (const cleaner of catalog.cleaners) {
      if (cleaner.running && keys.some((k) => k.startsWith(`${cleaner.id}.`))) {
        lines.push(
          `${cleaner.name} is running: files it has open will be skipped.`,
        );
      }
      for (const option of cleaner.options) {
        if (option.warning && keys.includes(key(cleaner.id, option.id))) {
          lines.push(`${cleaner.name} › ${option.name}: ${option.warning}`);
        }
      }
    }
    const answer = await confirm({
      title: "Clean now?",
      message: settings.shred
        ? `What the ${plural(keys.length, "ticked option")} find will be overwritten and permanently deleted. This cannot be undone.`
        : `What the ${plural(keys.length, "ticked option")} find will be permanently deleted (not moved to the ${trashName()}).`,
      ok: "Clean",
      danger: true,
      list: lines,
      checkbox: "Don't ask again",
    });
    if (!answer) return false;
    // The clean goes ahead either way: the person did confirm it.
    if (answer.checked)
      await ctx
        .update({ confirm: false })
        .catch((e: unknown) =>
          notice(`"Don't ask again" could not be saved: ${String(e)}`, true),
        );
    return true;
  };

  const run = async (mode: Mode) => {
    const keys = selected();
    if (!keys.length || running) return;
    if (mode === "clean" && !(await confirmClean(keys))) return;
    running = true;
    stop.hidden = false;
    updateStatus();
    const card = new ProgressCard(
      mode === "clean" ? "Cleaning…" : "Looking for what to clean…",
    );
    results.replaceChildren(card.element);
    try {
      const report: Report = await api.runCleaner(mode, keys, (p) =>
        card.update(p),
      );
      results.replaceChildren(
        ...renderReport(report, ctx.info.elevated, {
          restartAsAdmin: () =>
            api.restartAsAdmin().catch((e: unknown) => notice(String(e), true)),
        }),
      );
      if (mode === "clean") {
        if (ctx.settings().exitAfterClean && !report.cancelled) {
          await api.quitApp();
          return;
        }
        await load();
      }
    } catch (error) {
      results.replaceChildren(
        el(
          "div",
          { class: "banner danger" },
          el("span", { class: "banner-text", text: String(error) }),
        ),
      );
    } finally {
      running = false;
      stop.hidden = true;
      updateStatus();
    }
  };

  preview.addEventListener("click", () => void run("preview"));
  clean.addEventListener("click", () => void run("clean"));
  stop.addEventListener("click", () => {
    api.cancelJob().catch((e: unknown) => notice(String(e), true));
  });
  selectAll.addEventListener("click", () => void tickAll());
  selectDefaults.addEventListener("click", () => {
    if (catalog) ticked = defaults(catalog.cleaners);
    changed();
  });
  selectNone.addEventListener("click", () => {
    ticked = new Set();
    changed();
  });
  showAll.addEventListener("change", () => {
    ctx
      .update({ hideIrrelevant: !showAll.checked })
      .then(redraw)
      .catch((e: unknown) => notice(String(e), true));
  });
  ctx.onSettings(() => redraw());

  const element = el(
    "section",
    { class: "page cleaner", id: "page-cleaner" },
    el(
      "div",
      { class: "tree-pane" },
      tree.element,
      el(
        "div",
        { class: "tree-tools" },
        el("span", { text: "Select:" }),
        selectAll,
        selectDefaults,
        selectNone,
        el(
          "label",
          { class: "check" },
          showAll,
          el("span", { text: "Show all" }),
        ),
        shownCount,
      ),
    ),
    results,
  );
  idle();

  return {
    element,
    commands: [stop, preview, clean],
    onShow: () => {
      if (!running) void load();
    },
    onSearch: (q) => {
      query = q;
      redraw();
    },
  };
};
