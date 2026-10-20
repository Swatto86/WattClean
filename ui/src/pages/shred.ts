// Shred files: overwrite chosen files and folders, then delete them.

import { api, type ShredReport } from "../api.ts";
import type { Context, Page } from "../context.ts";
import { confirm } from "../dialog.ts";
import { commandButton, el, glyph, notice } from "../dom.ts";
import { bytes, count, plural } from "../format.ts";
import { diskEncryption, samePath } from "../platform.ts";

export const shredPage = (_ctx: Context): Page => {
  const queue: string[] = [];
  let running = false;

  const addFiles = commandButton(
    "shred-add-files",
    "Add files",
    "file",
    "secondary",
  );
  const addFolder = commandButton(
    "shred-add-folder",
    "Add folder",
    "folder",
    "secondary",
  );
  const clear = commandButton("shred-clear", "Clear list", "close", "command");
  const start = commandButton(
    "shred-start",
    "Shred",
    "shred",
    "primary danger",
  );
  const stop = commandButton("shred-stop", "Stop", "stop", "secondary");
  stop.hidden = true;

  const list = el("ul", { class: "list", id: "shred-list" });
  const outcome = el("div", { id: "shred-outcome" });
  const status = el("p", { class: "muted", id: "shred-status" });

  const draw = () => {
    list.replaceChildren(
      ...(queue.length
        ? queue.map((path, index) => {
            const remove = el(
              "button",
              {
                type: "button",
                class: "icon-button",
                "aria-label": `Remove ${path}`,
              },
              glyph("close", "glyph small"),
            );
            remove.addEventListener("click", () => {
              queue.splice(index, 1);
              draw();
            });
            return el(
              "li",
              {},
              el("span", { class: "path", text: path, title: path }),
              remove,
            );
          })
        : [
            el("li", {
              class: "empty",
              text: "Nothing to shred yet. Add files or a folder.",
            }),
          ]),
    );
    start.disabled = running || queue.length === 0;
    clear.disabled = running || queue.length === 0;
    addFiles.disabled = running;
    addFolder.disabled = running;
  };

  const add = async (folder: boolean) => {
    try {
      for (const path of await api.pickToShred(folder)) {
        if (!queue.some((p) => samePath(p, path))) queue.push(path);
      }
      draw();
    } catch (error) {
      notice(String(error), true);
    }
  };

  const show = (report: ShredReport) => {
    const parts = [
      `${plural(report.files, "file")} and ${plural(report.folders, "folder")} shredded (${bytes(report.bytes)}).`,
    ];
    if (report.cancelled) parts.push("Stopped before the end.");
    outcome.replaceChildren(
      el(
        "div",
        { class: report.problems.length ? "banner" : "card" },
        el("span", { class: "banner-text", text: parts.join(" ") }),
      ),
    );
    if (report.problems.length) {
      outcome.append(
        el(
          "ul",
          { class: "problems" },
          ...report.problems.map((p) =>
            el("li", { text: `${p.path}: ${p.message}` }),
          ),
        ),
      );
    }
  };

  const shred = async () => {
    if (!queue.length || running) return;
    const answer = await confirm({
      title: "Shred these?",
      message: `${plural(queue.length, "item")} will be overwritten and deleted. Folders go with everything in them. Shredded files cannot be recovered by any means.`,
      list: queue
        .slice(0, 12)
        .concat(queue.length > 12 ? [`and ${queue.length - 12} more`] : []),
      ok: "Shred",
      danger: true,
    });
    if (!answer) return;
    running = true;
    stop.hidden = false;
    draw();
    outcome.replaceChildren();
    try {
      const report = await api.shred([...queue], (p) => {
        status.textContent = `${count(p.files)} files · ${bytes(p.bytes)} — ${p.current}`;
      });
      // What is still there stays queued to try again.
      const left = queue.filter((item) => report.left.includes(item));
      queue.splice(0, queue.length, ...left);
      show(report);
    } catch (error) {
      notice(String(error), true);
    } finally {
      running = false;
      stop.hidden = true;
      status.textContent = "";
      draw();
    }
  };

  addFiles.addEventListener("click", () => void add(false));
  addFolder.addEventListener("click", () => void add(true));
  clear.addEventListener("click", () => {
    queue.length = 0;
    draw();
  });
  start.addEventListener("click", () => void shred());
  stop.addEventListener("click", () => {
    api.cancelJob().catch((e: unknown) => notice(String(e), true));
  });

  const element = el(
    "section",
    { class: "page", id: "page-shred" },
    el(
      "div",
      { class: "scroll" },
      el(
        "div",
        { class: "card" },
        el("h2", { text: "Delete files beyond recovery" }),
        el("p", {
          class: "muted",
          text: `Each file is renamed, overwritten with zeros, emptied and then deleted, so recovery tools cannot bring it back. On SSDs and USB sticks the drive itself may keep old copies out of reach of any program; ${diskEncryption()} is the reliable answer there.`,
        }),
      ),
      list,
      status,
      outcome,
    ),
  );
  draw();
  return { element, commands: [stop, clear, addFiles, addFolder, start] };
};
