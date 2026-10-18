/** The installed-programs list: search, sort, keyboard selection, details. */
import { api, type Program } from "./api.ts";
import { byId, clear, h } from "./dom.ts";
import { avatarColour, initial, kilobytes } from "./format.ts";
import { step, view, type Sort, type SortKey } from "./listing.ts";

const SCOPES: Record<Program["scope"], string> = {
  machine64: "64-bit",
  machine32: "32-bit",
  user: "This user",
};

export class ProgramList {
  private programs: Program[] = [];
  private shown: Program[] = [];
  private selected: string | null = null;
  private sort: Sort = { key: "name", ascending: true };
  private loading = 0;
  private query = "";
  /** Lower-case ids of programs installed with tracing. */
  private traced = new Set<string>();

  private readonly rows = byId("program-rows", HTMLTableSectionElement);
  private readonly search = byId("search", HTMLInputElement);
  private readonly count = byId("program-count", HTMLParagraphElement);
  private readonly details = byId("details", HTMLElement);
  private readonly uninstallButton = byId("uninstall", HTMLButtonElement);

  constructor(private readonly onUninstall: (program: Program) => void) {
    // `change` too: clearing the box programmatically fires no `input`.
    for (const kind of ["input", "change"]) {
      this.search.addEventListener(kind, () => {
        // `change` also fires on blur; rebuilding the rows then would
        // swallow the click that caused the blur.
        if (this.search.value === this.query) return;
        this.query = this.search.value;
        this.render();
      });
    }
    byId("refresh", HTMLButtonElement).addEventListener(
      "click",
      () => void this.load(),
    );
    this.uninstallButton.addEventListener("click", () =>
      this.uninstallSelected(),
    );
    for (const header of document.querySelectorAll<HTMLTableCellElement>(
      "th[data-sort]",
    )) {
      header
        .querySelector("button")
        ?.addEventListener("click", () => this.sortBy(header));
    }
    this.rows.addEventListener("keydown", (event) => this.keydown(event));
  }

  async load(): Promise<void> {
    const ticket = ++this.loading;
    this.count.textContent = "Loading…";
    try {
      const [programs, logs] = await Promise.all([
        api.listInstalled(),
        api.listLogs().catch(() => []),
      ]);
      this.traced = new Set(
        logs.flatMap((log) =>
          log.programId ? [log.programId.toLowerCase()] : [],
        ),
      );
      if (ticket !== this.loading) return;
      this.programs = programs;
      this.render();
    } catch (error: unknown) {
      if (ticket === this.loading)
        this.count.textContent = `Could not read the program list: ${String(error)}`;
    }
  }

  current(): Program | null {
    return (
      this.programs.find((program) => program.id === this.selected) ?? null
    );
  }

  private sortBy(header: HTMLTableCellElement): void {
    const key = header.dataset["sort"];
    if (
      key !== "name" &&
      key !== "publisher" &&
      key !== "version" &&
      key !== "size" &&
      key !== "date"
    )
      return;
    const sortKey: SortKey = key;
    this.sort = {
      key: sortKey,
      ascending:
        this.sort.key === sortKey
          ? !this.sort.ascending
          : sortKey === "name" || sortKey === "publisher",
    };
    for (const other of document.querySelectorAll("th[data-sort]"))
      other.removeAttribute("aria-sort");
    header.setAttribute(
      "aria-sort",
      this.sort.ascending ? "ascending" : "descending",
    );
    this.render();
  }

  private render(): void {
    this.shown = view(this.programs, this.search.value, this.sort);
    const total = this.programs.length;
    this.count.textContent =
      this.shown.length === total
        ? `${total} programs`
        : `${this.shown.length} of ${total} programs`;
    // A refresh replaces the rows; keep keyboard focus in the list if it was there.
    const hadFocus = this.rows.contains(document.activeElement);
    clear(this.rows);
    for (const program of this.shown) this.rows.append(this.row(program));
    if (!this.shown.some((program) => program.id === this.selected))
      this.selected = null;
    // Keep one row reachable with Tab even before anything is selected.
    const first = this.rows.querySelector<HTMLTableRowElement>("tr");
    if (first && this.selected === null) first.tabIndex = 0;
    this.showDetails();
    if (hadFocus) {
      const target = this.selected ?? this.shown[0]?.id;
      if (target !== undefined) this.select(target, true);
    }
  }

  private row(program: Program): HTMLTableRowElement {
    const avatar = h(
      "span",
      { class: "avatar", "aria-hidden": "true" },
      initial(program.name),
    );
    avatar.style.background = avatarColour(program.name);
    const name = h(
      "td",
      { title: program.name },
      h(
        "span",
        { class: "name-cell" },
        avatar,
        h("span", {}, program.name),
        h("span", { class: "chip" }, SCOPES[program.scope]),
        program.msiProductCode ? h("span", { class: "chip" }, "MSI") : null,
        program.systemComponent ? h("span", { class: "chip" }, "System") : null,
        this.traced.has(program.id.toLowerCase())
          ? h("span", { class: "chip traced" }, "Traced")
          : null,
      ),
    );
    const row = h(
      "tr",
      {
        tabindex: "-1",
        "aria-selected": String(program.id === this.selected),
        "data-id": program.id,
      },
      name,
      h("td", { title: program.publisher ?? "" }, program.publisher ?? ""),
      h("td", {}, program.version ?? ""),
      h("td", { class: "num" }, kilobytes(program.sizeKb)),
      h("td", {}, program.installDate ?? ""),
    );
    row.addEventListener("click", () => this.select(program.id, false));
    row.addEventListener("focus", () => {
      if (this.selected !== program.id) this.select(program.id, false);
    });
    row.addEventListener("dblclick", () => this.onUninstall(program));
    return row;
  }

  private select(id: string, focus: boolean): void {
    this.selected = id;
    for (const row of this.rows.querySelectorAll<HTMLTableRowElement>("tr")) {
      const chosen = row.dataset["id"] === id;
      row.setAttribute("aria-selected", String(chosen));
      row.tabIndex = chosen ? 0 : -1;
      if (chosen && focus) {
        row.focus();
        row.scrollIntoView({ block: "nearest" });
      }
    }
    this.showDetails();
  }

  private keydown(event: KeyboardEvent): void {
    const index = this.shown.findIndex(
      (program) => program.id === this.selected,
    );
    if (event.key === "Enter" || event.key === "Delete") {
      event.preventDefault();
      this.uninstallSelected();
      return;
    }
    const next = step(index, this.shown.length, event.key);
    const program = this.shown[next];
    if (next !== index && program) {
      event.preventDefault();
      this.select(program.id, true);
    }
  }

  private uninstallSelected(): void {
    const program = this.current();
    if (program) this.onUninstall(program);
  }

  private showDetails(): void {
    const program = this.current();
    this.uninstallButton.disabled = program === null;
    clear(this.details);
    if (!program) return;
    const facts: Array<[string, string | null]> = [
      ["Name", program.name],
      ["Publisher", program.publisher],
      ["Version", program.version],
      ["Install folder", program.installLocation],
      ["Uninstall command", program.uninstallCommand],
      [
        "Registry key",
        `${program.registryKey.hive}\\${program.registryKey.path}`,
      ],
      ["Website", program.website],
      ["Comments", program.comments],
    ];
    for (const [label, value] of facts) {
      if (!value) continue;
      this.details.append(h("dt", {}, label), h("dd", {}, value));
    }
  }
}
