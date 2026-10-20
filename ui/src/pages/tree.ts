// The cleaner tree: cleaners grouped by category, each with its options.
// Built once per filter; ticking updates the boxes in place.

import type { Category, CleanerView } from "../api.ts";
import { el, glyph } from "../dom.ts";
import { machineName, osName } from "../platform.ts";
import { cleanerState, key, matches } from "../selection.ts";

/** Each category's heading, in the tree's order. */
export const groups = (): Record<Category, string> => ({
  system: osName(),
  packages: "Package managers",
  browsers: "Browsers",
  applications: "Applications",
  developer: "Developer tools",
  games: "Games",
  "deep-scan": "Deep scan",
  custom: "Custom cleaners",
  community: "Community (winapp2.ini)",
});

export interface TreeEvents {
  /** An option's box was clicked; resolves whether to keep the new state. */
  option: (
    cleaner: CleanerView,
    optionId: string,
    ticked: boolean,
  ) => Promise<boolean>;
  /** A cleaner's own box was clicked. */
  cleaner: (cleaner: CleanerView) => void;
}

export class Tree {
  readonly element = el("div", {
    class: "tree",
    id: "cleaner-tree",
    role: "tree",
  });
  private cleanerBoxes = new Map<
    string,
    { box: HTMLInputElement; tally: HTMLElement; cleaner: CleanerView }
  >();
  private optionBoxes = new Map<string, HTMLInputElement>();
  private expanded = new Set<string>();

  constructor(private readonly events: TreeEvents) {}

  /** Draws the cleaners that pass the filters; returns how many are shown. */
  render(
    cleaners: CleanerView[],
    ticked: ReadonlySet<string>,
    hideIrrelevant: boolean,
    query: string,
  ): number {
    this.cleanerBoxes.clear();
    this.optionBoxes.clear();
    const shown = cleaners.filter(
      (c) =>
        (c.present || !hideIrrelevant) &&
        matches(
          c,
          query,
          c.options.map((o) => o.name),
        ),
    );
    const nodes: HTMLElement[] = [];
    for (const [category, label] of Object.entries(groups())) {
      const members = shown.filter((c) => c.category === category);
      if (members.length === 0) continue;
      nodes.push(el("div", { class: "group-title", text: label }));
      for (const cleaner of members)
        nodes.push(...this.cleanerNodes(cleaner, query !== ""));
    }
    if (nodes.length === 0) {
      nodes.push(el("p", { class: "muted", text: "No cleaner matches." }));
    }
    this.element.replaceChildren(...nodes);
    this.sync(ticked);
    return shown.length;
  }

  /** Updates every box to match `ticked`. */
  sync(ticked: ReadonlySet<string>): void {
    for (const [k, box] of this.optionBoxes) box.checked = ticked.has(k);
    for (const { box, tally, cleaner } of this.cleanerBoxes.values()) {
      const state = cleanerState(cleaner, ticked);
      box.checked = state === "on";
      box.indeterminate = state === "mixed";
      const on = cleaner.options.filter((o) =>
        ticked.has(key(cleaner.id, o.id)),
      ).length;
      tally.textContent = on ? `${on}/${cleaner.options.length}` : "";
    }
  }

  private cleanerNodes(
    cleaner: CleanerView,
    searching: boolean,
  ): HTMLElement[] {
    const open = searching || this.expanded.has(cleaner.id);
    const expander = el("button", {
      type: "button",
      class: "expander",
      "aria-label": `Show ${cleaner.name}'s options`,
      "aria-expanded": String(open),
    });
    const box = el("input", {
      type: "checkbox",
      id: `clean-${cleaner.id}`,
      "aria-label": `All of ${cleaner.name}`,
    });
    box.addEventListener("click", () => this.events.cleaner(cleaner));
    const tally = el("span", { class: "tally" });
    this.cleanerBoxes.set(cleaner.id, { box, tally, cleaner });
    const name = el("span", {
      class: "cleaner-name",
      text: cleaner.name,
      title: cleaner.description,
    });
    const row = el(
      "div",
      {
        class: cleaner.present ? "cleaner-row" : "cleaner-row absent",
        "data-cleaner": cleaner.id,
        title: cleaner.present
          ? cleaner.description
          : `${cleaner.description} (not found on this ${machineName()})`,
      },
      expander,
      box,
      name,
      cleaner.running
        ? el("span", { class: "pill warn", text: "Running" })
        : null,
      tally,
    );
    const options = cleaner.options.map((option) => {
      const k = key(cleaner.id, option.id);
      const optionBox = el("input", {
        type: "checkbox",
        id: `opt-${cleaner.id}-${option.id}`,
        "data-key": k,
      });
      optionBox.addEventListener("click", () => {
        const wanted = optionBox.checked;
        this.events
          .option(cleaner, option.id, wanted)
          .then((keep) => {
            if (!keep) optionBox.checked = !wanted;
          })
          .catch(() => {
            optionBox.checked = !wanted;
          });
      });
      this.optionBoxes.set(k, optionBox);
      const label = el(
        "label",
        { class: "option-row", for: optionBox.id, hidden: !open },
        optionBox,
        el(
          "span",
          { class: "option-text" },
          el("span", { text: option.name }),
          el("span", { class: "option-desc", text: option.description }),
        ),
        option.warning ? glyph("warning", "glyph small warn-mark") : null,
      );
      if (option.warning) label.title = option.warning;
      return label;
    });
    const toggle = () => {
      const now = expander.getAttribute("aria-expanded") !== "true";
      expander.setAttribute("aria-expanded", String(now));
      if (now) this.expanded.add(cleaner.id);
      else this.expanded.delete(cleaner.id);
      for (const option of options) option.hidden = !now;
    };
    expander.addEventListener("click", toggle);
    name.addEventListener("click", toggle);
    return [row, ...options];
  }
}
