// An editable list of files and folders in the settings (the keep list
// and Custom paths): add through an Open dialog, remove with a click.

import { api, type PathEntry, type SettingsPatch } from "../api.ts";
import type { Context } from "../context.ts";
import { failure } from "../decode.ts";
import { el, glyph, notice } from "../dom.ts";

export const pathList = (
  ctx: Context,
  save: (patch: SettingsPatch) => Promise<void>,
  id: string,
  field: "keep" | "customPaths",
  empty: string,
) => {
  const list = el("ul", { class: "list", id });
  const draw = (entries: PathEntry[]) => {
    list.replaceChildren(
      ...(entries.length
        ? entries.map((entry, index) => {
            const remove = el(
              "button",
              {
                type: "button",
                class: "icon-button",
                "aria-label": `Remove ${entry.path}`,
              },
              glyph("close", "glyph small"),
            );
            remove.addEventListener("click", () => {
              const next = ctx.settings()[field].filter((_, i) => i !== index);
              void save({ [field]: next });
            });
            return el(
              "li",
              {},
              glyph(entry.folder ? "folder" : "file", "glyph small"),
              el("span", {
                class: "path",
                text: entry.path,
                title: entry.path,
              }),
              remove,
            );
          })
        : [el("li", { class: "empty", text: empty })]),
    );
  };
  const add = (folder: boolean) => async () => {
    try {
      const path = await api.pickPath(folder);
      if (path)
        await save({ [field]: [...ctx.settings()[field], { path, folder }] });
    } catch (error) {
      notice(failure(error), true);
    }
  };
  const addFile = el(
    "button",
    { type: "button", class: "secondary", id: `${id}-add-file` },
    glyph("file", "glyph small"),
    el("span", { text: "Add file" }),
  );
  const addFolder = el(
    "button",
    { type: "button", class: "secondary", id: `${id}-add-folder` },
    glyph("folder", "glyph small"),
    el("span", { text: "Add folder" }),
  );
  addFile.addEventListener("click", () => void add(false)());
  addFolder.addEventListener("click", () => void add(true)());
  return {
    list,
    draw,
    buttons: el("div", { class: "row-buttons" }, addFile, addFolder),
  };
};
