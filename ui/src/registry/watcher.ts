/** The Registry Watcher view: watched keys on the left, their log on the right. */
import {
  api,
  keyText,
  message,
  type TraceState,
  type WatchEvent,
  type WatchView,
} from "./api.ts";
import type { AppContext } from "./context.ts";
import { confirm, inform } from "./dialog.ts";
import { byId, h } from "./dom.ts";
import { copyText, count, timeOfDay } from "./format.ts";
import { item, onMenu, popup, separator } from "./menu.ts";

const OP_TEXT: Record<WatchEvent["op"], string> = {
  keyCreated: "Key created",
  keyDeleted: "Key deleted",
  valueAdded: "Value added",
  valueChanged: "Value changed",
  valueDeleted: "Value deleted",
  valueSet: "Value set",
};
/** The table shows at most this many changes; Export saves them all. */
const SHOWN = 2000;
/** The page holds at most this many, for the filter to search; Export saves them all. */
const KEPT = 20_000;

export interface WatcherView {
  load: () => Promise<void>;
}

/** The program in a log row: its name, or its number when Windows gave none. */
function processText(event: WatchEvent): string {
  if (!event.actor) return "";
  return event.actor.process || `PID ${event.actor.pid}`;
}

export function wireWatcher(context: AppContext): WatcherView {
  const listing = byId("watch-list", HTMLUListElement);
  const rows = byId("watch-events", HTMLTableSectionElement);
  const filter = byId("watch-filter", HTMLInputElement);
  const exportButton = byId("watch-export", HTMLButtonElement);
  const clearButton = byId("watch-clear", HTMLButtonElement);
  let watches: WatchView[] = [];
  let selected: string | null = null;
  let log: WatchEvent[] = [];
  let tracing: TraceState | null = null;

  const run = (work: () => Promise<unknown>) =>
    void work().catch((error: unknown) =>
      inform("That did not work", message(error)),
    );

  const renderList = () => {
    if (watches.length === 0) {
      listing.replaceChildren(
        h("li", { class: "empty" }, "No key is watched yet."),
      );
      return;
    }
    listing.replaceChildren(
      ...watches.map((watch) => {
        const key = keyText(watch.key);
        const button = h(
          "button",
          {
            type: "button",
            class: "watch-item",
            "aria-pressed": String(key === selected),
          },
          h("code", {}, key),
          h(
            "small",
            { class: "muted" },
            `${watch.paused ? "Paused" : "Watching"} · ${count(watch.events, "change", "changes")}`,
          ),
          watch.problem ? h("small", { class: "error" }, watch.problem) : null,
        );
        button.addEventListener("click", () => run(() => select(key)));
        return h("li", { "data-key": key }, button);
      }),
    );
  };

  /** Say why the Process column is empty, when it is. */
  const renderTracing = (state: TraceState) => {
    if (state.state === tracing?.state && state.problem === tracing.problem)
      return;
    tracing = state;
    const note = byId("watch-tracing", HTMLParagraphElement);
    note.hidden = state.state === "on" || state.state === "idle";
    note.classList.toggle("bad", state.state === "failed");
    if (state.state === "needsAdmin") {
      const restart = h(
        "button",
        { class: "ghost", type: "button" },
        "Restart as administrator",
      );
      restart.addEventListener("click", () => context.restartAsAdmin());
      note.replaceChildren(
        "Which program made each change is shown only when WattClean runs as administrator.",
        restart,
      );
    } else {
      note.replaceChildren(state.problem ?? "");
    }
  };

  const renderLog = () => {
    const words = filter.value.trim().toLowerCase();
    const shown = log
      .filter((event) =>
        words === ""
          ? true
          : [
              OP_TEXT[event.op],
              processText(event),
              keyText(event.key),
              event.value,
              event.old,
              event.new,
            ]
              .join(" ")
              .toLowerCase()
              .includes(words),
      )
      .slice(-SHOWN)
      .reverse();
    rows.replaceChildren(
      ...shown.map((event) =>
        h(
          "tr",
          { "data-seq": String(event.seq) },
          h("td", {}, timeOfDay(event.time)),
          h(
            "td",
            { title: event.actor ? `Process ID ${event.actor.pid}` : "" },
            processText(event),
          ),
          h("td", {}, OP_TEXT[event.op]),
          h("td", { title: keyText(event.key) }, keyText(event.key)),
          h("td", {}, event.value ?? ""),
          h("td", { title: event.old ?? "" }, event.old ?? ""),
          h("td", { title: event.new ?? "" }, event.new ?? ""),
        ),
      ),
    );
    exportButton.disabled = selected === null;
    clearButton.disabled = selected === null;
  };

  const select = async (key: string | null) => {
    selected = key;
    const events = key ? await api.watchEvents(key, 0) : [];
    // Another key was chosen while this one's log was read.
    if (selected !== key) return;
    log = events.slice(-KEPT);
    renderList();
    renderLog();
  };

  const load = async () => {
    renderTracing(await api.watchTracing());
    watches = await api.listWatches();
    const keys = watches.map((watch) => keyText(watch.key));
    const keep = selected !== null && keys.includes(selected) ? selected : null;
    await select(keep ?? keys[0] ?? null);
  };

  /**
   * New changes for the selected key, and fresh counts, every second while
   * the page is on show (not while another page is, or the window is in
   * the notification area).
   */
  const poll = async () => {
    if (document.hidden || !byId("view-watcher", HTMLElement).checkVisibility())
      return;
    const key = selected;
    if (key) {
      const last = log[log.length - 1]?.seq ?? 0;
      const fresh = await api.watchEvents(key, last);
      // Only into the log of the key they were read for.
      if (fresh.length > 0 && selected === key) {
        log = [...log, ...fresh].slice(-KEPT);
        renderLog();
      }
    }
    const listed = await api.listWatches();
    // Redrawn only when something changed, so focus in the list stays put.
    if (JSON.stringify(listed) !== JSON.stringify(watches)) {
      watches = listed;
      renderList();
    }
    renderTracing(await api.watchTracing());
  };
  setInterval(() => void poll().catch(() => undefined), 1000);

  byId("watch-form", HTMLFormElement).addEventListener("submit", (event) => {
    event.preventDefault();
    const input = byId("watch-key", HTMLInputElement);
    const error = byId("watch-error", HTMLParagraphElement);
    error.textContent = "";
    void (async () => {
      try {
        const added = await api.addWatch(input.value);
        input.value = "";
        watches = await api.listWatches();
        await select(keyText(added));
      } catch (failure: unknown) {
        error.textContent = message(failure);
      }
    })();
  });
  filter.addEventListener("input", renderLog);
  const exportLog = async () => {
    if (!selected) return;
    const saved = await api.exportWatch(selected);
    if (saved) await inform("Log saved", `Saved to ${saved}`);
  };
  const clearLog = async () => {
    if (!selected) return;
    await api.clearWatch(selected);
    log = [];
    renderLog();
  };
  exportButton.addEventListener("click", () => run(exportLog));
  clearButton.addEventListener("click", () => run(clearLog));

  onMenu("watches", (target, at) => {
    const key = target.closest<HTMLElement>("li[data-key]")?.dataset["key"];
    const watch = watches.find((w) => keyText(w.key) === key);
    if (!key || !watch) return;
    run(async () => {
      await select(key);
      const chosen = await popup(
        [
          item("pause", watch.paused ? "&Resume watching" : "&Pause watching"),
          item("remove", "&Stop watching"),
          separator,
          item("regedit", "&Open in Registry Editor"),
          item("copy", "Cop&y key path"),
        ],
        at,
      );
      if (chosen === "pause") await api.pauseWatch(key, !watch.paused);
      if (chosen === "remove") {
        const yes = await confirm(
          "Stop watching",
          `Stop watching ${key} and drop its log?`,
          "Stop watching",
          "danger",
        );
        if (yes) await api.removeWatch(key);
      }
      if (chosen === "regedit") await api.openInRegedit(key);
      if (chosen === "copy") await copyText(key);
      await load();
    });
  });
  onMenu("events", (target, at) => {
    const seq = Number(
      target.closest<HTMLElement>("tr[data-seq]")?.dataset["seq"],
    );
    const event = log.find((e) => e.seq === seq);
    run(async () => {
      const chosen = await popup(
        [
          item("regedit", "&Open in Registry Editor", event !== undefined),
          item("copy", "Cop&y key path", event !== undefined),
          item("data", "Copy new &data", event?.new != null),
          separator,
          item("export", "E&xport log…", selected !== null),
          item("clear", "&Clear log", selected !== null),
        ],
        at,
      );
      if (chosen === "regedit" && event)
        await api.openInRegedit(keyText(event.key));
      if (chosen === "copy" && event) await copyText(keyText(event.key));
      if (chosen === "data" && event?.new) await copyText(event.new);
      if (chosen === "export") await exportLog();
      if (chosen === "clear") await clearLog();
    });
  });
  return { load };
}
