// The right-hand side of the Cleaner: what a preview found or a clean did.

import type { ItemKind, OptionReport, Progress, Report } from "../api.ts";
import { el, glyph } from "../dom.ts";
import { bytes, count, duration, percent, plural } from "../format.ts";
import {
  fullDiskAccessBanner,
  isWindows,
  platform,
  runningBanner,
} from "../platform.ts";

const KIND_LABELS: Record<ItemKind, string> = {
  file: "File",
  folder: "Folder",
  truncate: "Emptied",
  registry: "Registry",
  database: "Database",
  json: "Setting",
  ini: "Setting",
  "recycle-bin": "Bin",
  clipboard: "Clipboard",
  "free-space": "Free space",
  command: "Command",
};

const kindLabel = (kind: ItemKind): string =>
  kind === "recycle-bin" && !isWindows() ? "Trash" : KIND_LABELS[kind];

const tile = (label: string, value: string, extra = ""): HTMLElement =>
  el(
    "div",
    { class: `tile ${extra}`.trim() },
    el("span", { class: "tile-label", text: label }),
    el("span", { class: "tile-value", text: value }),
  );

/** "12 files · 3 folders · 2 registry entries" for one option. */
const summary = (r: OptionReport): string => {
  const c = r.counts;
  const parts: string[] = [];
  if (c.files) parts.push(plural(c.files, "file"));
  if (c.folders) parts.push(plural(c.folders, "folder"));
  if (c.registry)
    parts.push(plural(c.registry, "registry entry", "registry entries"));
  if (c.other) parts.push(plural(c.other, "other item"));
  if (c.problems) parts.push(plural(c.problems, "problem"));
  return parts.length ? parts.join(" · ") : "nothing found";
};

const optionResult = (r: OptionReport): HTMLElement => {
  const head = el(
    "button",
    {
      type: "button",
      class: "result-head",
      "aria-expanded": "false",
      "data-key": r.key,
    },
    el("span", { class: "expander", "aria-hidden": "true" }),
    el(
      "span",
      { class: "result-name" },
      el("strong", { text: r.cleaner }),
      el("span", { class: "muted", text: ` › ${r.option}` }),
    ),
    el("span", { class: "result-count", text: summary(r) }),
    el("span", { class: "result-size", text: bytes(r.counts.bytes) }),
  );
  const list = el("ul", { class: "items", hidden: true });
  let filled = false;
  head.addEventListener("click", () => {
    const open = head.getAttribute("aria-expanded") !== "true";
    head.setAttribute("aria-expanded", String(open));
    const marker = head.querySelector(".expander");
    marker?.setAttribute("aria-expanded", String(open));
    if (open && !filled) {
      filled = true;
      for (const item of r.items) {
        list.append(
          el(
            "li",
            {},
            el("span", { class: "item-kind", text: kindLabel(item.kind) }),
            el("span", {
              class: "item-path",
              text: item.note ? `${item.path}  (${item.note})` : item.path,
              title: item.path,
            }),
            el("span", {
              class: "item-size",
              text: item.size ? bytes(item.size) : "",
            }),
          ),
        );
      }
      if (r.more > 0) {
        list.append(
          el(
            "li",
            { class: "more" },
            el("span"),
            el("span", { text: `and ${count(r.more)} more not listed` }),
            el("span"),
          ),
        );
      }
      for (const problem of r.problems) {
        list.append(
          el(
            "li",
            { class: "problem" },
            el("span", { class: "item-kind", text: "Problem" }),
            el("span", {
              class: "item-path",
              text: `${problem.path}: ${problem.message}`,
              title: problem.path,
            }),
            el("span"),
          ),
        );
      }
      if (!list.childElementCount)
        list.append(
          el(
            "li",
            { class: "more" },
            el("span"),
            el("span", { text: "Nothing here." }),
            el("span"),
          ),
        );
    }
    list.hidden = !open;
  });
  return el("div", { class: "result" }, head, list);
};

export interface ReportActions {
  restartAsAdmin: () => void;
  /** macOS: System Settings at Full Disk Access. */
  openPrivacySettings: () => void;
}

/** What macOS's privacy protection kept from WattClean: the engine's
 *  problems carry a Full Disk Access hint on macOS only. */
const protectedItems = (report: Report): number =>
  report.options.reduce(
    (sum, o) =>
      sum +
      o.problems.filter((p) => p.message.includes("Full Disk Access")).length,
    0,
  );

/** The whole report: banners, totals and each option's findings. */
export const renderReport = (
  report: Report,
  elevated: boolean,
  actions: ReportActions,
): HTMLElement[] => {
  const nodes: HTMLElement[] = [];
  const cleaning = report.mode === "clean";
  if (report.cancelled) {
    nodes.push(
      el(
        "div",
        { class: "banner" },
        el("span", {
          class: "banner-text",
          text: "Stopped before the end: what is listed was done, nothing else.",
        }),
      ),
    );
  }
  if (report.running.length) {
    nodes.push(
      el(
        "div",
        { class: "banner", id: "running-banner" },
        glyph("warning"),
        el("span", {
          class: "banner-text",
          text: runningBanner(report.running),
        }),
      ),
    );
  }
  const needsAdmin = report.options.some((o) =>
    o.problems.some((p) => p.message.includes("administrator")),
  );
  if (needsAdmin && !elevated) {
    // Linux and macOS ask for administrator rights per command instead.
    const button = el("button", {
      type: "button",
      class: "secondary",
      text: "Restart as administrator",
      hidden: !isWindows(),
    });
    button.addEventListener("click", actions.restartAsAdmin);
    nodes.push(
      el(
        "div",
        { class: "banner" },
        el("span", {
          class: "banner-text",
          text: isWindows()
            ? "Some items need administrator rights."
            : "Some items need administrator rights, which were not granted.",
        }),
        button,
      ),
    );
  }
  const shielded = platform() === "macos" ? protectedItems(report) : 0;
  if (shielded > 0) {
    const button = el("button", {
      type: "button",
      class: "secondary",
      text: "Open Privacy & Security",
    });
    button.addEventListener("click", actions.openPrivacySettings);
    nodes.push(
      el(
        "div",
        { class: "banner", id: "privacy-banner" },
        el("span", {
          class: "banner-text",
          text: fullDiskAccessBanner(shielded),
        }),
        button,
      ),
    );
  }
  const t = report.totals;
  nodes.push(
    el(
      "div",
      { class: "tiles", id: "report-tiles" },
      tile(cleaning ? "Freed" : "Space to free", bytes(t.bytes), "big"),
      tile("Files", count(t.files)),
      tile("Folders", count(t.folders)),
      // Only Windows has a registry.
      isWindows() ? tile("Registry entries", count(t.registry)) : null,
      tile("Other items", count(t.other)),
      tile(
        t.inUse ? `Problems (${count(t.inUse)} in use)` : "Problems",
        count(t.problems),
        t.problems ? "bad" : "",
      ),
    ),
  );
  const verb = cleaning ? "Cleaned" : "Previewed";
  nodes.push(
    el("p", {
      class: "muted",
      id: "report-summary",
      text: `${verb} ${plural(report.options.length, "option")} in ${duration(report.elapsedMs)}.${cleaning ? "" : " Nothing has been deleted yet."}`,
    }),
  );
  const sorted = [...report.options].sort(
    (a, b) =>
      b.counts.bytes - a.counts.bytes || b.counts.files - a.counts.files,
  );
  nodes.push(
    el(
      "div",
      { class: "option-results", id: "option-results" },
      ...sorted.map(optionResult),
    ),
  );
  return nodes;
};

/** The card shown while a preview or clean runs. */
export class ProgressCard {
  readonly element: HTMLElement;
  private label = el("span", { class: "progress-label" });
  private counter = el("span", { class: "muted" });
  private fill = el("div", { class: "bar-fill" });

  constructor(title: string) {
    this.element = el(
      "div",
      { class: "card progress", id: "progress" },
      el("h2", { text: title }),
      el("div", { class: "progress-head" }, this.label, this.counter),
      el("div", { class: "bar" }, this.fill),
    );
  }

  update(progress: Progress): void {
    this.label.textContent = progress.label || "Finishing…";
    this.counter.textContent = `${count(progress.items)} items · ${bytes(progress.bytes)}`;
    this.fill.style.width = `${percent(progress.done, progress.total)}%`;
  }
}
