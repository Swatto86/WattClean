/** The list of found leftovers with a tick box each, grouped by kind. */
import type { Leftover, ScanReport } from "./api.ts";
import { h } from "./dom.ts";
import { bytes, plural, target } from "./format.ts";

const LEVELS: Record<Leftover["confidence"], [string, string]> = {
  high: ["Certain", "ok"],
  medium: ["Likely", "warn"],
  low: ["Possible", "bad"],
};

export class LeftoverList {
  readonly element: HTMLDivElement;
  private readonly boxes = new Map<number, HTMLInputElement>();
  private readonly summary: HTMLParagraphElement;

  constructor(
    private readonly report: ScanReport,
    private readonly onChange: () => void,
  ) {
    this.summary = h("p", {
      class: "muted",
      role: "status",
      id: "leftover-summary",
    });
    const registry = report.items.filter(
      (item) => item.target.kind === "key" || item.target.kind === "value",
    );
    const files = report.items.filter(
      (item) => item.target.kind === "folder" || item.target.kind === "file",
    );
    const controls = h(
      "div",
      { class: "toolbar" },
      this.button("Select all", "select-all", () => this.setAll(true)),
      this.button("Select none", "select-none", () => this.setAll(false)),
    );
    this.element = h(
      "div",
      { class: "leftovers" },
      ...report.warnings.map((warning) => h("p", { class: "notice" }, warning)),
      controls,
      this.summary,
      this.group("Registry", registry),
      this.group("Files and folders", files),
    );
    this.update();
  }

  selectedIds(): number[] {
    return [...this.boxes].filter(([, box]) => box.checked).map(([id]) => id);
  }

  private button(
    label: string,
    id: string,
    act: () => void,
  ): HTMLButtonElement {
    const button = h("button", { class: "ghost", type: "button", id }, label);
    button.addEventListener("click", act);
    return button;
  }

  private group(title: string, items: Leftover[]): HTMLElement | null {
    if (items.length === 0) return null;
    return h(
      "section",
      { class: "leftover-group" },
      h("h3", {}, `${title} (${items.length})`),
      ...items.map((item) => this.row(item)),
    );
  }

  private row(item: Leftover): HTMLLabelElement {
    const box = h("input", { type: "checkbox", "data-id": String(item.id) });
    box.checked = item.selected;
    box.addEventListener("change", () => this.update());
    this.boxes.set(item.id, box);
    const [label, tone] = LEVELS[item.confidence];
    const size = item.bytes > 0 ? ` · ${bytes(item.bytes)}` : "";
    return h(
      "label",
      { class: "leftover" },
      box,
      h("span", { class: "path" }, target(item.target)),
      h("span", { class: `pill ${tone} level` }, label),
      h("span", { class: "why" }, `${item.reason}${size}`),
    );
  }

  private setAll(checked: boolean): void {
    for (const box of this.boxes.values()) box.checked = checked;
    this.update();
  }

  private update(): void {
    const chosen = new Set(this.selectedIds());
    const size = this.report.items
      .filter((item) => chosen.has(item.id))
      .reduce((sum, item) => sum + item.bytes, 0);
    this.summary.textContent = `${chosen.size} of ${plural(this.report.items.length, "item")} selected${size > 0 ? ` · ${bytes(size)} on disk` : ""}`;
    this.onChange();
  }
}
