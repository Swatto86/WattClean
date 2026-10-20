// History: what past cleans, shredding and wipes did.

import { api, type HistoryEntry } from "../api.ts";
import type { Context, Page } from "../context.ts";
import { failure } from "../decode.ts";
import { confirm } from "../dialog.ts";
import { commandButton, el, glyph, notice } from "../dom.ts";
import { bytes, count, when } from "../format.ts";

const ACTIVITY: Record<HistoryEntry["activity"], string> = {
  clean: "Clean",
  shred: "Shred",
  wipe: "Wipe",
};

export const historyPage = (_ctx: Context): Page => {
  const clear = commandButton(
    "history-clear",
    "Clear history",
    "clean",
    "command",
  );
  const body = el("div", { class: "scroll", id: "history" });

  const load = async () => {
    try {
      const entries = await api.getHistory();
      if (!entries.length) {
        body.replaceChildren(
          el(
            "div",
            { class: "card hero" },
            glyph("history"),
            el("h2", { text: "Nothing yet" }),
            el("p", {
              text: "Each clean, shred and wipe is listed here with what it freed.",
            }),
          ),
        );
        clear.disabled = true;
        return;
      }
      clear.disabled = false;
      const total = entries
        .filter((e) => e.activity === "clean")
        .reduce((sum, e) => sum + e.bytes, 0);
      body.replaceChildren(
        el(
          "div",
          { class: "tiles" },
          el(
            "div",
            { class: "tile big" },
            el("span", {
              class: "tile-label",
              text: "Freed by cleaning, all listed",
            }),
            el("span", {
              class: "tile-value",
              id: "history-total",
              text: bytes(total),
            }),
          ),
          el(
            "div",
            { class: "tile" },
            el("span", { class: "tile-label", text: "Entries" }),
            el("span", { class: "tile-value", text: count(entries.length) }),
          ),
        ),
        el(
          "table",
          { class: "table", id: "history-table" },
          el(
            "thead",
            {},
            el(
              "tr",
              {},
              el("th", { text: "When" }),
              el("th", { text: "What" }),
              el("th", { text: "Details" }),
              el("th", { class: "end", text: "Items" }),
              el("th", { class: "end", text: "Size" }),
              el("th", { class: "end", text: "Problems" }),
            ),
          ),
          el(
            "tbody",
            {},
            ...entries.map((e) =>
              el(
                "tr",
                {},
                el("td", { text: when(e.at) }),
                el("td", {
                  text: e.cancelled
                    ? `${ACTIVITY[e.activity]} (stopped)`
                    : ACTIVITY[e.activity],
                }),
                el("td", { text: e.summary }),
                el("td", {
                  class: "end",
                  text: e.activity === "wipe" ? "" : count(e.items),
                }),
                el("td", { class: "end", text: bytes(e.bytes) }),
                el("td", {
                  class: "end",
                  text: e.problems ? count(e.problems) : "",
                }),
              ),
            ),
          ),
        ),
      );
    } catch (error) {
      notice(failure(error), true);
    }
  };

  clear.addEventListener("click", () => {
    void (async () => {
      const answer = await confirm({
        title: "Clear the history?",
        message: "The list is emptied; nothing else changes.",
        ok: "Clear",
      });
      if (!answer) return;
      try {
        await api.clearHistory();
        await load();
      } catch (error) {
        notice(failure(error), true);
      }
    })();
  });

  const element = el("section", { class: "page", id: "page-history" }, body);
  return { element, commands: [clear], onShow: () => void load() };
};
