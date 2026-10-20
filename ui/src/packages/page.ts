// Uninstall programs on Linux and macOS: what the package managers and
// Applications folders hold. Uninstalling runs the package manager's own
// removal (which may ask for an administrator's password), then offers what
// the program left in the home folder and the background jobs that start
// it; nothing is ticked, each can be shown in the file manager first, and
// what is ticked goes to the Trash.

import type { Context, Page } from "../context.ts";
import { confirm, inform } from "../dialog.ts";
import { commandButton, el, glyph, notice } from "../dom.ts";
import { bytes, count, plural } from "../format.ts";
import { trashName } from "../platform.ts";
import { packagesApi, type LeftoverView, type PackageView } from "./api.ts";
import { removalHow, revealLabel } from "./text.ts";

/** What "also remove its settings" means for each source, where it can. */
const PURGE: Record<string, string> = {
  APT: "Also remove its system-wide settings files (purge)",
  pacman: "Also remove its saved settings and dependencies nothing else needs",
  Flatpak: "Also delete its data",
  Snap: "Also delete its data without keeping a snapshot",
  "Homebrew Cask":
    "Also run Homebrew's zap: removes its settings and caches, and may remove files other apps share, including outside your home folder",
};

const matches = (p: PackageView, query: string): boolean =>
  [p.name, p.summary ?? "", p.source, p.key].some((text) =>
    text.toLowerCase().includes(query),
  );

/** Asks which leftovers to move to the Trash; resolves with their numbers. */
const chooseLeftovers = (
  name: string,
  leftovers: LeftoverView[],
): Promise<number[]> =>
  new Promise((resolve) => {
    const boxes = leftovers.map((leftover) => {
      const box = el("input", {
        type: "checkbox",
        id: `un-pkg-left-${leftover.number}`,
      });
      return { box, leftover };
    });
    const shown = el("p", {
      class: "muted",
      id: "un-pkg-reveal-status",
      role: "status",
    });
    const show = (leftover: LeftoverView) => {
      const button = el("button", {
        type: "button",
        class: "link",
        id: `un-pkg-show-${leftover.number}`,
        text: "Show",
        title: revealLabel(),
        "aria-describedby": `un-pkg-path-${leftover.number}`,
      });
      button.addEventListener("click", () => {
        shown.textContent = "";
        packagesApi.reveal(leftover.number).catch((error: unknown) => {
          shown.textContent = String(error);
        });
      });
      return button;
    };
    const form = el(
      "form",
      { method: "dialog", class: "dialog-form" },
      el("h2", {
        class: "dialog-title",
        id: "dialog-title",
        text: `What ${name} left behind`,
      }),
      el("p", {
        class: "dialog-text",
        text: `Tick what to move to the ${trashName()}. Only items named after ${name}, and background jobs that start it, are listed; check each one, since another program may share a name. A background job is stopped before it moves.`,
      }),
      el(
        "ul",
        { class: "list", id: "un-pkg-leftovers" },
        ...boxes.map(({ box, leftover }) =>
          el(
            "li",
            {},
            el(
              "label",
              { class: "check", for: box.id },
              box,
              el("span", {
                class: "path",
                id: `un-pkg-path-${leftover.number}`,
                text: leftover.path,
              }),
            ),
            el("span", {
              class: "muted",
              text:
                (leftover.job ? "background job, " : "") +
                (leftover.size === null
                  ? "size unknown"
                  : bytes(leftover.size)),
            }),
            show(leftover),
          ),
        ),
      ),
      shown,
      el(
        "div",
        { class: "dialog-buttons" },
        el("button", {
          type: "submit",
          value: "ok",
          class: "primary",
          text: `Move to the ${trashName()}`,
        }),
        el("button", {
          type: "submit",
          value: "cancel",
          class: "secondary",
          text: "Keep them",
        }),
      ),
    );
    const dialog = el(
      "dialog",
      { class: "dialog", "aria-labelledby": "dialog-title" },
      form,
    );
    let chosen: number[] = [];
    form.addEventListener("submit", (event) => {
      const submitter = event.submitter;
      if (submitter instanceof HTMLButtonElement && submitter.value === "ok") {
        chosen = boxes
          .filter(({ box }) => box.checked)
          .map(({ leftover }) => leftover.number);
      }
    });
    dialog.addEventListener("close", () => {
      dialog.remove();
      resolve(chosen);
    });
    document.body.append(dialog);
    dialog.showModal();
  });

export const programsPage = (_ctx: Context): Page => {
  const uninstall = commandButton(
    "un-pkg-uninstall",
    "Uninstall",
    "programs",
    "primary danger",
  );
  const refresh = commandButton("un-pkg-refresh", "Refresh", "refresh");
  const all = el("input", { type: "checkbox", id: "un-pkg-all" });
  const source = el("select", { id: "un-pkg-source", "aria-label": "Source" });
  const status = el("span", { class: "muted", id: "un-pkg-status" });
  const body = el("tbody", {});
  const table = el(
    "table",
    { class: "table selectable", id: "un-pkg-table" },
    el(
      "thead",
      {},
      el(
        "tr",
        {},
        el("th", { text: "Program" }),
        el("th", { text: "Source" }),
        el("th", { text: "Version" }),
        el("th", { class: "end", text: "Size" }),
      ),
    ),
    body,
  );
  let packages: PackageView[] = [];
  let selected: string | null = null;
  let query = "";
  /** A list is being loaded. */
  let busy = false;
  /** A removal is running; only the removal itself clears this. */
  let removing = false;
  /** The latest list load; an older one's answer is dropped. */
  let loading = 0;

  const selectedPackage = () => packages.find((p) => p.key === selected);
  const sync = () => {
    const idle = !busy && !removing;
    uninstall.disabled = !idle || !selectedPackage();
    refresh.disabled = !idle;
    all.disabled = !idle;
    source.disabled = !idle;
  };

  const draw = () => {
    const from = source.value;
    const shown = packages.filter(
      (p) => (from === "" || p.source === from) && matches(p, query),
    );
    body.replaceChildren(
      ...shown.map((p) => {
        const row = el(
          "tr",
          { "aria-selected": String(p.key === selected), tabindex: 0 },
          el(
            "td",
            {},
            el("div", { text: p.name }),
            p.summary
              ? el("div", { class: "muted small", text: p.summary })
              : null,
          ),
          el("td", { text: p.source }),
          el("td", { text: p.version ?? "" }),
          el("td", {
            class: "end",
            text: p.size === null ? "" : bytes(p.size),
          }),
        );
        const pick = () => {
          selected = p.key;
          for (const other of body.querySelectorAll("tr")) {
            other.setAttribute("aria-selected", String(other === row));
          }
          sync();
        };
        row.addEventListener("click", pick);
        row.addEventListener("keydown", (event) => {
          if (event.key === " " || event.key === "Enter") {
            event.preventDefault();
            pick();
          }
        });
        return row;
      }),
    );
    status.textContent =
      shown.length === packages.length
        ? plural(packages.length, all.checked ? "package" : "program")
        : `${count(shown.length)} of ${plural(packages.length, all.checked ? "package" : "program")}`;
    sync();
  };

  const load = async () => {
    const ticket = ++loading;
    busy = true;
    sync();
    status.textContent = "Asking the package managers…";
    try {
      const list = await packagesApi.list(all.checked);
      if (ticket !== loading) return;
      packages = list.packages;
      const sources = [...new Set(packages.map((p) => p.source))].sort();
      const chosen = source.value;
      source.replaceChildren(
        el("option", { value: "", text: "Every source" }),
        ...sources.map((s) => el("option", { value: s, text: s })),
      );
      source.value = sources.includes(chosen) ? chosen : "";
      if (!packages.some((p) => p.key === selected)) selected = null;
      for (const problem of list.problems) notice(problem, true);
    } catch (error) {
      if (ticket === loading) notice(String(error), true);
    } finally {
      if (ticket === loading) {
        busy = false;
        draw();
      }
    }
  };

  const remove = async () => {
    const target = selectedPackage();
    if (!target) return;
    const purge = PURGE[target.source];
    const answer = await confirm({
      title: `Uninstall ${target.name}?`,
      message: `${removalHow(target.source)} Then WattClean looks for what it left in your home folder.`,
      ok: "Uninstall",
      danger: true,
      ...(purge ? { checkbox: purge } : {}),
    });
    if (!answer) return;
    removing = true;
    sync();
    status.textContent = `Removing ${target.name}…`;
    try {
      const done = await packagesApi.uninstall(
        target.key,
        purge !== undefined && answer.checked,
      );
      if (done.leftovers.length === 0) {
        notice(`${done.name} was removed. Nothing was left behind.`);
      } else {
        const numbers = await chooseLeftovers(done.name, done.leftovers);
        if (numbers.length > 0) {
          const trashed = await packagesApi.trashLeftovers(numbers);
          if (trashed.failed.length > 0) {
            await inform(
              "Some leftovers were not moved",
              `${plural(trashed.moved, "item")} went to the ${trashName()}; these could not:`,
              trashed.failed,
            );
          } else {
            notice(
              `${plural(trashed.moved, "item")} moved to the ${trashName()}.`,
            );
          }
        } else {
          notice(`${done.name} was removed; its leftovers were kept.`);
        }
      }
    } catch (error) {
      notice(String(error), true);
    } finally {
      removing = false;
      await load();
    }
  };

  uninstall.addEventListener("click", () => void remove());
  refresh.addEventListener("click", () => void load());
  all.addEventListener("change", () => void load());
  source.addEventListener("change", draw);

  const element = el(
    "section",
    { class: "page", id: "page-programs" },
    el(
      "div",
      { class: "scroll" },
      el(
        "div",
        { class: "bar" },
        el(
          "label",
          { class: "check", for: "un-pkg-all" },
          all,
          el("span", { text: "Show every package (libraries and tools too)" }),
        ),
        source,
        status,
      ),
      table,
      el(
        "p",
        { class: "muted" },
        glyph("shield"),
        ` Removing uses each package manager's own command. Leftovers go to the ${trashName()}, where you can restore them.`,
      ),
    ),
  );
  let loaded = false;
  return {
    element,
    commands: [uninstall, refresh],
    onShow: () => {
      if (!loaded) {
        loaded = true;
        void load();
      }
    },
    onSearch: (text) => {
      query = text.trim().toLowerCase();
      draw();
    },
  };
};
