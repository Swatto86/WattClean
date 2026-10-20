// Uninstall programs on Linux and macOS: what the package managers and
// Applications folders hold. Uninstalling runs the package manager's own
// removal (which may ask for an administrator's password), then offers what
// the program left in the home folder and the background jobs that start
// it (`leftovers.ts`). On macOS "Leftovers of removed apps" offers the
// background jobs of apps already gone the same way.

import type { Context, Page } from "../context.ts";
import { confirm } from "../dialog.ts";
import { commandButton, el, glyph, notice } from "../dom.ts";
import { bytes, count, plural } from "../format.ts";
import { platform, trashName } from "../platform.ts";
import { packagesApi, type PackageView } from "./api.ts";
import { offerLeftovers } from "./leftovers.ts";
import { leftoversIntro, orphansIntro, removalHow } from "./text.ts";

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

export const programsPage = (_ctx: Context): Page => {
  const uninstall = commandButton(
    "un-pkg-uninstall",
    "Uninstall",
    "programs",
    "primary danger",
  );
  const refresh = commandButton("un-pkg-refresh", "Refresh", "refresh");
  // Background jobs outlive an app dragged to the Trash on macOS; Linux's
  // package managers remove their own services.
  const macos = platform() === "macos";
  const orphans = commandButton(
    "un-pkg-orphans",
    "Leftovers of removed apps",
    "search",
  );
  const all = el("input", { type: "checkbox", id: "un-pkg-all" });
  const source = el("select", { id: "un-pkg-source", "aria-label": "Source" });
  const status = el("span", { class: "muted", id: "un-pkg-status" });
  // A package manager that could not list what it installed is named for
  // as long as the list lacks its programs.
  const problems = el("div", {
    class: "banner danger",
    id: "un-pkg-problems",
    role: "alert",
    hidden: true,
  });
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
  /** A removal, or the search for removed apps' leftovers, is running;
   *  only it clears this. */
  let removing = false;
  /** The latest list load; an older one's answer is dropped. */
  let loading = 0;

  const selectedPackage = () => packages.find((p) => p.key === selected);
  const sync = () => {
    const idle = !busy && !removing;
    uninstall.disabled = !idle || !selectedPackage();
    orphans.disabled = !idle;
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
      problems.replaceChildren(
        el("span", {
          class: "banner-text",
          text: list.problems.map((problem) => `${problem}.`).join(" "),
        }),
      );
      problems.hidden = list.problems.length === 0;
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
      message: `${removalHow(target.source)} Then WattClean looks for what it left behind.`,
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
        await offerLeftovers(
          `What ${done.name} left behind`,
          leftoversIntro(done.name, done.leftovers),
          done.leftovers,
          `${done.name} was removed; its leftovers were kept.`,
        );
      }
    } catch (error) {
      notice(String(error), true);
    } finally {
      removing = false;
      await load();
    }
  };

  const findOrphans = async () => {
    removing = true;
    sync();
    status.textContent = "Looking for leftovers of removed apps…";
    try {
      const found = await packagesApi.orphans();
      if (!found.spotlight) {
        notice(
          "Spotlight could not be asked, so only background jobs whose app folder is gone were looked for.",
          true,
        );
      }
      if (found.leftovers.length === 0) {
        notice("No leftovers of removed apps were found.");
      } else {
        await offerLeftovers(
          "Leftovers of removed apps",
          orphansIntro(found.leftovers),
          found.leftovers,
          "The leftovers were kept.",
        );
      }
    } catch (error) {
      notice(String(error), true);
    } finally {
      removing = false;
      draw();
    }
  };

  uninstall.addEventListener("click", () => void remove());
  orphans.addEventListener("click", () => void findOrphans());
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
        { class: "filters" },
        el(
          "label",
          { class: "check", for: "un-pkg-all" },
          all,
          el("span", { text: "Show every package (libraries and tools too)" }),
        ),
        source,
        status,
      ),
      problems,
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
    commands: macos ? [uninstall, orphans, refresh] : [uninstall, refresh],
    onShow: () => {
      if (!loaded) {
        loaded = true;
        void load();
      }
    },
    search: {
      placeholder: "Find a program",
      run: (text) => {
        query = text.trim().toLowerCase();
        draw();
      },
    },
  };
};
