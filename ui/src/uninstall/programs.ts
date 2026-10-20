/**
 * The installed-programs list: search, sort, details, and selection as in
 * Windows lists (Ctrl+click, Shift+click, Shift+arrows, Ctrl+A), so several
 * programs can be uninstalled one after another.
 */
import { api, type Program } from "./api.ts";
import { byId, clear, h } from "./dom.ts";
import { avatarColour, initial, kilobytes, plural } from "./format.ts";
import { range, step, view, type Sort, type SortKey } from "./listing.ts";

/** The keys that move through the list. */
const MOVES = new Set([
  "ArrowUp",
  "ArrowDown",
  "Home",
  "End",
  "PageUp",
  "PageDown",
]);

const SCOPES: Record<Program["scope"], string> = {
  machine64: "64-bit",
  machine32: "32-bit",
  user: "This user",
};

export class ProgramList {
  private programs: Program[] = [];
  private shown: Program[] = [];
  /** Ids of the selected programs. */
  private selection = new Set<string>();
  /** The row the keyboard is on, and where a Shift selection starts. */
  private focused: string | null = null;
  private anchor: string | null = null;
  /** True while the list moves focus itself. */
  private focusing = false;
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

  /** `onUninstall` gets the chosen programs in list order. */
  constructor(private readonly onUninstall: (programs: Program[]) => void) {
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
        this.count.textContent = `Could not read the program list: ${error instanceof Error ? error.message : String(error)}`;
    }
  }

  /** The selected programs, in list order. */
  private chosen(): Program[] {
    return this.shown.filter((program) => this.selection.has(program.id));
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
    // Only programs on show stay selected: Uninstall never acts on hidden ones.
    const ids = new Set(this.shown.map((program) => program.id));
    this.selection = new Set([...this.selection].filter((id) => ids.has(id)));
    if (this.focused !== null && !ids.has(this.focused)) this.focused = null;
    if (this.anchor !== null && !ids.has(this.anchor)) this.anchor = null;
    // The keyboard carries on from a selected row when its own was hidden.
    this.focused ??= this.chosen()[0]?.id ?? null;
    clear(this.rows);
    for (const program of this.shown) this.rows.append(this.row(program));
    this.mark();
    if (hadFocus) {
      const target = this.focused ?? this.shown[0]?.id;
      if (target === undefined) return;
      if (this.selection.size === 0) this.selectOnly(target);
      this.focusRow(target, true);
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
      { tabindex: "-1", "aria-selected": "false", "data-id": program.id },
      name,
      h("td", { title: program.publisher ?? "" }, program.publisher ?? ""),
      h("td", {}, program.version ?? ""),
      h("td", { class: "num" }, kilobytes(program.sizeKb)),
      h("td", {}, program.installDate ?? ""),
    );
    // A click moves focus itself, after the selection is set, so a
    // Ctrl+click into an empty selection is not selected twice.
    row.addEventListener("mousedown", (event) => event.preventDefault());
    row.addEventListener("click", (event) => this.click(program.id, event));
    // Tabbing into an empty selection selects the row. Focus moved by the
    // list itself (after a click or a key) has set the selection already.
    row.addEventListener("focus", () => {
      if (this.focusing) return;
      if (this.selection.size === 0) this.selectOnly(program.id);
      this.focusRow(program.id, false);
    });
    row.addEventListener("dblclick", (event) => {
      if (event.ctrlKey || event.metaKey || event.shiftKey) return;
      this.onUninstall([program]);
    });
    return row;
  }

  private click(id: string, event: MouseEvent): void {
    if (event.shiftKey) {
      const span = range(this.ids(), this.anchor, id);
      this.selection = new Set(
        event.ctrlKey || event.metaKey ? [...this.selection, ...span] : span,
      );
    } else if (event.ctrlKey || event.metaKey) {
      if (!this.selection.delete(id)) this.selection.add(id);
      this.anchor = id;
    } else {
      this.selectOnly(id);
    }
    this.focusRow(id, true);
  }

  private ids(): string[] {
    return this.shown.map((program) => program.id);
  }

  private selectOnly(id: string): void {
    this.selection = new Set([id]);
    this.anchor = id;
  }

  /** Makes `id` the keyboard's row, then redraws the selection. */
  private focusRow(id: string, focus: boolean): void {
    this.focused = id;
    this.mark();
    if (!focus) return;
    const row = this.rows.querySelector<HTMLTableRowElement>(
      `tr[data-id="${CSS.escape(id)}"]`,
    );
    this.focusing = true;
    row?.focus();
    this.focusing = false;
    row?.scrollIntoView({ block: "nearest" });
  }

  /** Shows the selection on the rows, the button and the details. */
  private mark(): void {
    const tabStop = this.focused ?? this.shown[0]?.id ?? null;
    for (const row of this.rows.querySelectorAll<HTMLTableRowElement>("tr")) {
      const id = row.dataset["id"] ?? "";
      row.setAttribute("aria-selected", String(this.selection.has(id)));
      row.tabIndex = id === tabStop ? 0 : -1;
    }
    this.showDetails();
  }

  private keydown(event: KeyboardEvent): void {
    const ids = this.ids();
    if (event.key === "Enter" || event.key === "Delete") {
      event.preventDefault();
      this.uninstallSelected();
      return;
    }
    if (event.key === "a" && (event.ctrlKey || event.metaKey)) {
      event.preventDefault();
      this.selection = new Set(ids);
      this.mark();
      return;
    }
    if (event.key === " " && this.focused !== null) {
      event.preventDefault();
      if (!this.selection.delete(this.focused))
        this.selection.add(this.focused);
      this.anchor = this.focused;
      this.mark();
      return;
    }
    // Tab and every other key keep their usual meaning.
    if (!MOVES.has(event.key)) return;
    const index = this.focused === null ? -1 : ids.indexOf(this.focused);
    const next = ids[step(index, ids.length, event.key)];
    if (next === undefined || next === this.focused) return;
    event.preventDefault();
    if (event.shiftKey) {
      this.selection = new Set(range(ids, this.anchor ?? next, next));
      if (this.anchor === null) this.anchor = next;
    } else if (!event.ctrlKey && !event.metaKey) {
      this.selectOnly(next);
    }
    this.focusRow(next, true);
  }

  private uninstallSelected(): void {
    const programs = this.chosen();
    if (programs.length > 0) this.onUninstall(programs);
  }

  private showDetails(): void {
    const programs = this.chosen();
    this.uninstallButton.disabled = programs.length === 0;
    this.uninstallButton.textContent =
      programs.length > 1
        ? `Uninstall ${plural(programs.length, "program")}`
        : "Uninstall";
    clear(this.details);
    const [program] = programs;
    if (!program) {
      this.details.append(
        h("dt", {}, "Tip"),
        h(
          "dd",
          {},
          "Ctrl+click or Shift+click selects several programs, which are then uninstalled one at a time.",
        ),
      );
      return;
    }
    if (programs.length > 1) {
      this.details.append(
        h("dt", {}, "Selected"),
        h(
          "dd",
          {},
          `${plural(programs.length, "program")}, uninstalled one at a time: ${programs.map((p) => p.name).join(", ")}`,
        ),
      );
      return;
    }
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
