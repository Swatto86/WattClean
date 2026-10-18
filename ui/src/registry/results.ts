/**
 * The findings tree of the cleaner and the search: one group per category,
 * each entry ticked or not, driven by mouse, keyboard and a native
 * right-click menu.
 */
import {
  api,
  CATEGORIES,
  keyText,
  message,
  type Category,
  type CategoryInfo,
  type Finding,
  type ResultList,
} from "./api.ts";
import { inform } from "./dialog.ts";
import { h } from "./dom.ts";
import { copyText } from "./format.ts";
import { item, onMenu, popup, separator, type Point } from "./menu.ts";

export interface TreeOptions {
  root: HTMLElement;
  list: ResultList;
  titles: Map<Category, CategoryInfo>;
  elevated: boolean;
  /** Called whenever the ticked entries change. */
  changed: () => void;
  /** Add a key to the exclude list (the tree then drops its entries). */
  exclude: (key: string) => Promise<void>;
}

const value = (finding: Finding) =>
  finding.value === null
    ? null
    : finding.value === ""
      ? "(Default)"
      : finding.value;

/** What a web search for a finding looks for: its value, or its key's name. */
function searchName(finding: Finding): string {
  const name = finding.value ?? finding.key.path.split("\\").pop() ?? "";
  return name.length > 60 ? `${name.slice(0, 60)}…` : name;
}

export class FindingsTree {
  private findings: Finding[] = [];
  private readonly ticked = new Set<number>();
  private readonly collapsed = new Set<Category>();

  constructor(private readonly options: TreeOptions) {
    options.root.dataset["menu"] = `findings-${options.list}`;
    options.root.setAttribute("role", "tree");
    options.root.classList.add("tree");
    options.root.addEventListener("keydown", (event) => this.keydown(event));
    onMenu(
      `findings-${options.list}`,
      (target, at) => void this.menu(target, at),
    );
  }

  show(findings: Finding[]): void {
    this.findings = findings;
    this.ticked.clear();
    for (const finding of findings) {
      if (finding.ticked && this.allowed(finding)) this.ticked.add(finding.id);
    }
    this.collapsed.clear();
    this.render();
  }

  count(): number {
    return this.findings.length;
  }

  tickedIds(): number[] {
    return this.findings
      .filter((finding) => this.ticked.has(finding.id))
      .map((finding) => finding.id);
  }

  /** Entries needing rights this copy of the app does not have. */
  lockedCount(): number {
    return this.findings.filter((finding) => !this.allowed(finding)).length;
  }

  setAll(on: boolean): void {
    for (const finding of this.findings) this.tick(finding, on);
    this.refresh();
  }

  remove(ids: number[]): void {
    const gone = new Set(ids);
    this.findings = this.findings.filter((finding) => !gone.has(finding.id));
    for (const id of ids) this.ticked.delete(id);
    this.render();
  }

  private allowed(finding: Finding): boolean {
    return this.options.elevated || !finding.needsAdmin;
  }

  private tick(finding: Finding, on: boolean): void {
    if (on && this.allowed(finding)) this.ticked.add(finding.id);
    else this.ticked.delete(finding.id);
  }

  private groups(): Map<Category, Finding[]> {
    const groups = new Map<Category, Finding[]>();
    for (const finding of this.findings) {
      const group = groups.get(finding.category) ?? [];
      group.push(finding);
      groups.set(finding.category, group);
    }
    return groups;
  }

  private render(): void {
    const focused = document.activeElement?.closest<HTMLElement>("[data-row]");
    const keep = focused?.dataset["row"];
    const rows: HTMLElement[] = [];
    for (const [category, findings] of this.groups()) {
      const open = !this.collapsed.has(category);
      const title = this.options.titles.get(category)?.title ?? category;
      const box = h("input", { type: "checkbox", tabindex: "-1" });
      box.addEventListener("change", () => {
        for (const finding of findings) this.tick(finding, box.checked);
        this.refresh();
      });
      const twisty = h("span", { class: "twisty", "aria-hidden": "true" });
      twisty.addEventListener("click", () => this.toggleGroup(category));
      const header = h(
        "div",
        {
          class: "group-row",
          role: "treeitem",
          "aria-expanded": String(open),
          "data-row": `group:${category}`,
          "data-category": category,
          tabindex: "-1",
        },
        twisty,
        box,
        h("strong", {}, title),
        h("span", { class: "count" }, `(${findings.length})`),
      );
      rows.push(header);
      if (!open) continue;
      for (const finding of findings) rows.push(this.row(finding));
    }
    this.options.root.replaceChildren(...rows);
    const first = this.options.root.querySelector<HTMLElement>("[data-row]");
    const again = keep
      ? this.options.root.querySelector<HTMLElement>(`[data-row="${keep}"]`)
      : null;
    (again ?? first)?.setAttribute("tabindex", "0");
    if (again && focused) again.focus();
    this.refresh();
  }

  private row(finding: Finding): HTMLElement {
    const allowed = this.allowed(finding);
    const box = h("input", {
      type: "checkbox",
      tabindex: "-1",
      disabled: !allowed,
      "aria-label": "Remove this entry",
    });
    box.addEventListener("change", () => {
      this.tick(finding, box.checked);
      this.refresh();
    });
    const name = value(finding);
    return h(
      "div",
      {
        class: "finding",
        role: "treeitem",
        "data-row": `finding:${finding.id}`,
        "data-id": String(finding.id),
        tabindex: "-1",
        title: allowed ? false : "Needs administrator rights",
      },
      box,
      h(
        "div",
        { class: "what" },
        h("code", { class: "path" }, keyText(finding.key)),
        name === null ? null : h("span", { class: "value" }, name),
      ),
      h(
        "div",
        { class: "tags" },
        finding.account ? h("span", { class: "chip" }, finding.account) : null,
        finding.needsAdmin ? h("span", { class: "chip admin" }, "Admin") : null,
      ),
      h("div", { class: "why" }, finding.detail),
    );
  }

  /** Bring checkboxes and counts in line with what is ticked. */
  private refresh(): void {
    for (const row of this.options.root.querySelectorAll<HTMLElement>(
      ".finding",
    )) {
      const on = this.ticked.has(Number(row.dataset["id"]));
      row.setAttribute("aria-checked", String(on));
      const box = row.querySelector("input");
      if (box) box.checked = on;
    }
    for (const [category, findings] of this.groups()) {
      const header = this.options.root.querySelector<HTMLElement>(
        `.group-row[data-category="${category}"]`,
      );
      const box = header?.querySelector("input");
      if (!header || !box) continue;
      const on = findings.filter((f) => this.ticked.has(f.id)).length;
      box.checked = on > 0 && on === findings.length;
      box.indeterminate = on > 0 && on < findings.length;
      header.setAttribute(
        "aria-checked",
        on === 0 ? "false" : on === findings.length ? "true" : "mixed",
      );
    }
    this.options.changed();
  }

  private toggleGroup(category: Category, open?: boolean): void {
    const now = open ?? this.collapsed.has(category);
    if (now) this.collapsed.delete(category);
    else this.collapsed.add(category);
    this.render();
  }

  private setExpanded(open: boolean): void {
    this.collapsed.clear();
    if (!open)
      for (const category of this.groups().keys()) this.collapsed.add(category);
    this.render();
  }

  private rows(): HTMLElement[] {
    return [...this.options.root.querySelectorAll<HTMLElement>("[data-row]")];
  }

  private focusRow(row: HTMLElement | undefined): void {
    if (!row) return;
    for (const other of this.rows()) other.setAttribute("tabindex", "-1");
    row.setAttribute("tabindex", "0");
    row.focus();
  }

  private findingOf(row: Element | null): Finding | undefined {
    const id = Number(row?.closest<HTMLElement>(".finding")?.dataset["id"]);
    return this.findings.find((finding) => finding.id === id);
  }

  private toggleRow(row: HTMLElement): void {
    const finding = this.findingOf(row);
    if (finding) {
      this.tick(finding, !this.ticked.has(finding.id));
    } else {
      const category = row.dataset["category"];
      const group = [...this.groups()].find(([c]) => c === category)?.[1] ?? [];
      const all = group.every((f) => this.ticked.has(f.id));
      for (const f of group) this.tick(f, !all);
    }
    this.refresh();
  }

  private keydown(event: KeyboardEvent): void {
    const rows = this.rows();
    const origin = event.target;
    if (!(origin instanceof Element)) return;
    const row = origin.closest<HTMLElement>("[data-row]");
    if (!row) return;
    const at = rows.indexOf(row);
    const category = CATEGORIES.find((c) => c === row.dataset["category"]);
    const moves: Record<string, number | undefined> = {
      ArrowDown: at + 1,
      ArrowUp: at - 1,
      Home: 0,
      End: rows.length - 1,
    };
    const target = moves[event.key];
    if (target !== undefined) {
      event.preventDefault();
      this.focusRow(rows[Math.max(0, Math.min(rows.length - 1, target))]);
    } else if (event.key === " ") {
      event.preventDefault();
      this.toggleRow(row);
    } else if (event.key === "ArrowLeft" && category) {
      event.preventDefault();
      this.toggleGroup(category, false);
    } else if (event.key === "ArrowRight" && category) {
      event.preventDefault();
      this.toggleGroup(category, true);
    }
  }

  private async menu(target: Element, at: Point): Promise<void> {
    const row = target.closest<HTMLElement>("[data-row]");
    if (!row) return;
    this.focusRow(row);
    const finding = this.findingOf(row);
    const on = finding
      ? this.ticked.has(finding.id)
      : row.getAttribute("aria-checked") === "true";
    const what = finding ? "this" : "this group";
    const entries = [
      item(
        "toggle",
        on ? `&Unselect ${what}` : `&Select ${what}`,
        !finding || this.allowed(finding),
      ),
      separator,
      item("all", "Select &all"),
      item("none", "Select &none"),
      separator,
      item("expand", "&Expand all"),
      item("collapse", "&Collapse all"),
    ];
    if (finding) {
      entries.push(
        separator,
        item("regedit", "&Open in Registry Editor"),
        item("copy", "Cop&y key path"),
        item("exclude", "Add key to e&xclude list"),
        item("web", `Search &Google for "${searchName(finding)}"`),
      );
    }
    entries.push(separator, item("export", "Export &list…"));
    const chosen = await popup(entries, at);
    await this.act(chosen, row, finding);
  }

  private async act(
    chosen: string | null,
    row: HTMLElement,
    finding: Finding | undefined,
  ): Promise<void> {
    try {
      if (chosen === "toggle") this.toggleRow(row);
      else if (chosen === "all") this.setAll(true);
      else if (chosen === "none") this.setAll(false);
      else if (chosen === "expand") this.setExpanded(true);
      else if (chosen === "collapse") this.setExpanded(false);
      else if (chosen === "regedit" && finding)
        await api.openInRegedit(keyText(finding.key));
      else if (chosen === "copy" && finding)
        await copyText(keyText(finding.key));
      else if (chosen === "exclude" && finding) {
        const key = keyText(finding.key);
        await this.options.exclude(key);
        this.remove(
          this.findings
            .filter((f) => keyText(f.key).toLowerCase() === key.toLowerCase())
            .map((f) => f.id),
        );
      } else if (chosen === "web" && finding) {
        await api.searchWeb(searchName(finding));
      } else if (chosen === "export") {
        const saved = await api.exportFindings(this.options.list);
        if (saved) await inform("List saved", `Saved to ${saved}`);
      }
    } catch (error: unknown) {
      await inform("That did not work", message(error));
    }
  }
}
