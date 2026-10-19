// Uninstall programs on Linux and macOS: what the package managers and
// Applications folders hold. Uninstalling runs the package manager's own
// removal (which may ask for an administrator's password), then offers what
// the program left in the home folder; nothing is ticked, and what is
// ticked goes to the Trash.

import type { Context, Page } from "../context.ts";
import { confirm, inform } from "../dialog.ts";
import { commandButton, el, glyph, notice } from "../dom.ts";
import { bytes, count, plural } from "../format.ts";
import { trashName } from "../platform.ts";
import { packagesApi, type LeftoverView, type PackageView } from "./api.ts";

/** What "also remove its settings" means for each source, where it can. */
const PURGE: Record<string, string> = {
  APT: "Also remove its system-wide settings files (purge)",
  pacman: "Also remove its saved settings and dependencies nothing else needs",
  Flatpak: "Also delete its data",
  Snap: "Also delete its data without keeping a snapshot",
  "Homebrew Cask": "Also remove its settings and caches (zap)",
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
        text: `Tick what to move to the ${trashName()}. Only folders named after ${name} are listed; check each one, since another program may share a name.`,
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
              el("span", { class: "path", text: leftover.path }),
            ),
            el("span", {
              class: "muted",
              text:
                leftover.size === null ? "size unknown" : bytes(leftover.size),
            }),
          ),
        ),
      ),
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
  let busy = false;

  const selectedPackage = () => packages.find((p) => p.key === selected);
  const sync = () => {
    uninstall.disabled = busy || !selectedPackage();
    refresh.disabled = busy;
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
    busy = true;
    sync();
    status.textContent = "Asking the package managers…";
    try {
      const list = await packagesApi.list(all.checked);
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
      notice(String(error), true);
    } finally {
      busy = false;
      draw();
    }
  };

  const remove = async () => {
    const target = selectedPackage();
    if (!target) return;
    const purge = PURGE[target.source];
    const how =
      target.source === "Application"
        ? `Finder moves it to the ${trashName()}, asking for an administrator's password if it is not yours to move.`
        : target.source === "AppImage"
          ? `The file moves to the ${trashName()}.`
          : `${target.source} removes it, asking for an administrator's password when it needs one.`;
    const answer = await confirm({
      title: `Uninstall ${target.name}?`,
      message: `${how} Then WattClean looks for what it left in your home folder.`,
      ok: "Uninstall",
      danger: true,
      ...(purge ? { checkbox: purge } : {}),
    });
    if (!answer) return;
    busy = true;
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
      busy = false;
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
