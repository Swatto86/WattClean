/** Traced installs: install with tracing, and the install logs it saved. */
import { api, type LogSummary } from "./api.ts";
import { confirm, inform, Modal } from "./dialog.ts";
import { byId, clear, h } from "./dom.ts";
import { plural, target, when } from "./format.ts";
import { message, stepList } from "./wizard-view.ts";

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

/** Pick an installer, run it under tracing, save its log. */
export async function installWithTracing(onDone: () => void): Promise<void> {
  const path = await api.pickInstaller().catch(async (error: unknown) => {
    await inform("Could not choose an installer", message(error));
    return null;
  });
  if (path === null) return;
  let started: Awaited<ReturnType<typeof api.beginTrace>>;
  try {
    started = await api.beginTrace(path);
  } catch (error: unknown) {
    await inform("Tracing did not start", message(error));
    return;
  }
  const title = `Tracing ${started.installer}`;
  const modal = new Modal(title, true);
  let stop = false;
  modal.show(
    title,
    [
      stepList([
        [
          "Watching the installer",
          "busy",
          "Finish the installer's steps. Tracing stops by itself when it and everything it started have closed.",
        ],
      ]),
    ],
    [
      {
        label: "It has finished — stop tracing",
        value: "stop",
        id: "trace-stop",
      },
    ],
    () => {
      stop = true;
    },
  );
  while (!stop) {
    await sleep(500);
    if (await api.traceFinished(started.id).catch(() => true)) break;
  }
  modal.show(
    title,
    [stepList([["Saving the install log", "busy", ""]])],
    [],
    () => undefined,
  );
  let body: HTMLElement[];
  try {
    const summary = await api.finishTrace(started.id);
    body = summaryText(summary);
  } catch (error: unknown) {
    body = [h("p", { class: "notice bad" }, message(error))];
  }
  const done = () => {
    modal.close();
    onDone();
  };
  modal.dismissWith(done);
  modal.show(
    "Install traced",
    body,
    [{ label: "Close", value: "close", kind: "primary", id: "trace-close" }],
    done,
  );
}

function summaryText(summary: LogSummary): HTMLElement[] {
  const name = summary.program ?? summary.installer;
  const nodes = [
    h(
      "p",
      { id: "trace-result" },
      `Recorded ${plural(summary.createdItems, "item")} that ${name} created` +
        (summary.changedItems > 0
          ? `, and ${plural(summary.changedItems, "existing item")} it changed.`
          : "."),
    ),
  ];
  if (summary.program === null) {
    nodes.push(
      h(
        "p",
        { class: "notice" },
        "No uninstall entry was registered, so this log is not linked to a program in the list.",
      ),
    );
  }
  if (summary.eventsLost > 0) {
    nodes.push(
      h(
        "p",
        { class: "notice" },
        `Windows dropped ${plural(summary.eventsLost, "event")} while tracing, so the log may be incomplete.`,
      ),
    );
  }
  return nodes;
}

export async function showLogs(): Promise<void> {
  const list = byId("log-list", HTMLDivElement);
  const logs = await api.listLogs().catch(() => []);
  clear(list);
  if (logs.length === 0) {
    list.append(
      h(
        "p",
        { class: "empty" },
        "No traced installs yet. Use Install with tracing to record one.",
      ),
    );
  }
  for (const log of logs) list.append(logCard(log));
}

function logCard(log: LogSummary): HTMLElement {
  const view = h(
    "button",
    { class: "ghost", type: "button", "data-action": "view" },
    "View",
  );
  view.addEventListener("click", () => void viewLog(log));
  const remove = h(
    "button",
    { class: "ghost", type: "button", "data-action": "delete" },
    "Delete",
  );
  remove.addEventListener("click", () => void deleteLog(log));
  return h(
    "article",
    { class: "card entry", "data-log": log.id },
    h("div", {}, h("b", {}, log.program ?? log.installer)),
    h("div", { class: "actions" }, view, remove),
    h(
      "div",
      { class: "meta" },
      `${when(log.created)} · ${log.installer} · ${plural(log.createdItems, "item")} created` +
        (log.changedItems > 0 ? ` · ${log.changedItems} changed` : ""),
    ),
  );
}

async function viewLog(summary: LogSummary): Promise<void> {
  let log: Awaited<ReturnType<typeof api.getLog>>;
  try {
    log = await api.getLog(summary.id);
  } catch (error: unknown) {
    await inform("Could not open the log", message(error));
    return;
  }
  const rows = log.items.map((item) =>
    h(
      "div",
      { class: "leftover" },
      h("span", {}, ""),
      h("span", { class: "path" }, target(item.target)),
      h(
        "span",
        {
          class:
            item.recorded === "created" ? "pill ok level" : "pill warn level",
        },
        item.recorded === "created" ? "Created" : "Changed (kept)",
      ),
    ),
  );
  const modal = new Modal(
    `Install log of ${summary.program ?? summary.installer}`,
    true,
  );
  const close = () => modal.close();
  modal.dismissWith(close);
  modal.show(
    `Install log of ${summary.program ?? summary.installer}`,
    [
      h(
        "p",
        { class: "muted" },
        "Created items are removed with the program. Changed items existed before it was installed and are never removed.",
      ),
      ...rows,
    ],
    [{ label: "Close", value: "close", kind: "primary" }],
    close,
  );
}

async function deleteLog(log: LogSummary): Promise<void> {
  const sure = await confirm(
    "Delete install log?",
    `Delete the install log of ${log.program ?? log.installer}? Its uninstall will then rely on scanning alone.`,
    "Delete",
    "danger",
  );
  if (!sure) return;
  try {
    await api.deleteLog(log.id);
  } catch (error: unknown) {
    await inform("Delete failed", message(error));
  }
  await showLogs();
}
