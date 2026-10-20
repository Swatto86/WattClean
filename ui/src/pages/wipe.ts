// Wipe free space: fill a drive's free space with zeros so files deleted
// earlier cannot be recovered, then give the space back.

import { api, type Drive } from "../api.ts";
import type { Context, Page } from "../context.ts";
import { failure } from "../decode.ts";
import { confirm } from "../dialog.ts";
import { commandButton, el, glyph, notice } from "../dom.ts";
import { bytes, duration, percent } from "../format.ts";
import { noDriveToWipe, wipeCaveat } from "../platform.ts";

export const wipePage = (_ctx: Context): Page => {
  let running = false;
  const refresh = commandButton(
    "wipe-refresh",
    "Refresh",
    "refresh",
    "command",
  );
  const stop = commandButton("wipe-stop", "Stop", "stop", "secondary");
  stop.hidden = true;
  const drives = el("div", { class: "drives", id: "drives" });
  const none = el("p", {
    class: "muted",
    id: "wipe-none",
    text: noDriveToWipe(),
    hidden: true,
  });
  const progress = el("div", { id: "wipe-progress" });
  const outcome = el("div", { id: "wipe-outcome" });

  const driveCard = (drive: Drive): HTMLElement => {
    const used = drive.total - drive.free;
    const fill = el("div", { class: "bar-fill" });
    fill.style.width = `${percent(used, drive.total)}%`;
    const button = el("button", {
      type: "button",
      class: "secondary",
      text: "Wipe free space",
      "data-root": drive.root,
      disabled: running,
    });
    button.addEventListener("click", () => void wipe(drive));
    return el(
      "div",
      { class: "card drive" },
      el(
        "div",
        { class: "drive-head" },
        glyph("drive"),
        el(
          "div",
          {},
          el("div", {
            class: "drive-name",
            text: `${drive.label || (drive.removable ? "Removable drive" : "Local disk")} (${drive.root.replace(/\\$/, "")})`,
          }),
          el("div", {
            class: "muted",
            text: `${drive.fileSystem || "Unknown"} · ${bytes(drive.free)} free of ${bytes(drive.total)}`,
          }),
        ),
      ),
      el("div", { class: "bar" }, fill),
      el("div", { class: "row-buttons" }, button),
    );
  };

  const load = async () => {
    try {
      const list = await api.getDrives();
      drives.replaceChildren(...list.map(driveCard));
      none.hidden = list.length > 0;
    } catch (error) {
      notice(failure(error), true);
    }
  };

  const wipe = async (drive: Drive) => {
    if (running) return;
    const answer = await confirm({
      title: `Wipe the free space on ${drive.root}?`,
      message:
        `WattClean fills about ${bytes(drive.free)} with zeros and then deletes what it wrote. Your files are not touched. It can take a long time, and while it runs the drive is full, so programs that need to save may complain. On SSDs, TRIM usually clears freed space already. ${wipeCaveat(drive.removable)}`.trimEnd(),
      ok: "Wipe",
      danger: true,
    });
    if (!answer) return;
    running = true;
    stop.hidden = false;
    outcome.replaceChildren();
    const fill = el("div", { class: "bar-fill" });
    const label = el("span", { class: "progress-label", text: "Starting…" });
    progress.replaceChildren(
      el(
        "div",
        { class: "card progress" },
        el("h2", { text: `Wiping ${drive.root}` }),
        el("div", { class: "progress-head" }, label),
        el("div", { class: "bar" }, fill),
      ),
    );
    void load();
    try {
      const report = await api.wipeFreeSpace(drive.root, (p) => {
        fill.style.width = `${percent(p.written, p.target)}%`;
        label.textContent = `${bytes(p.written)} of about ${bytes(p.target)} written`;
      });
      outcome.replaceChildren(
        el(
          "div",
          { class: report.cancelled ? "banner" : "card", id: "wipe-result" },
          el("span", {
            class: "banner-text",
            text: `${report.cancelled ? "Stopped after writing" : "Wiped:"} ${bytes(report.written)} in ${duration(report.elapsedMs)}. The space has been given back.`,
          }),
        ),
      );
    } catch (error) {
      outcome.replaceChildren(
        el(
          "div",
          { class: "banner danger" },
          el("span", { class: "banner-text", text: failure(error) }),
        ),
      );
    } finally {
      running = false;
      stop.hidden = true;
      progress.replaceChildren();
      void load();
    }
  };

  refresh.addEventListener("click", () => void load());
  stop.addEventListener("click", () => {
    api.cancelJob().catch((e: unknown) => notice(failure(e), true));
  });

  const element = el(
    "section",
    { class: "page", id: "page-wipe" },
    el(
      "div",
      { class: "scroll" },
      el(
        "div",
        { class: "card" },
        el("h2", { text: "Make deleted files unrecoverable" }),
        el("p", {
          class: "muted",
          text: "Deleting a file only marks its space as free; the data stays on the disk until something overwrites it. Wiping free space overwrites all of it once, so nothing deleted before can be read back.",
        }),
      ),
      drives,
      none,
      progress,
      outcome,
    ),
  );
  return { element, commands: [stop, refresh], onShow: () => void load() };
};
