// Settings: the look and running in the background (general.ts), the disk
// cleaner (how cleaning behaves, the keep list, custom paths, community and
// custom cleaners), the registry cleaner's and the uninstaller's own
// settings, and about. Each change saves at once.

import { api, type Settings, type SettingsPatch } from "../api.ts";
import type { Context, Page } from "../context.ts";
import { failure } from "../decode.ts";
import { confirm } from "../dialog.ts";
import { el, notice } from "../dom.ts";
import { plural } from "../format.ts";
import { isWindows, machineName, osName } from "../platform.ts";
import { general } from "./general.ts";
import { pathList } from "./pathlist.ts";

const group = (
  title: string,
  id: string,
  ...children: (Node | null)[]
): HTMLFieldSetElement =>
  el(
    "fieldset",
    { class: "settings-group", id },
    el("legend", { text: title }),
    ...children,
  );

/** winapp2.ini describes Windows programs: offered on Windows only. */
const communityGroup = (
  ...args: Parameters<typeof group>
): HTMLFieldSetElement => {
  const fieldset = group(...args);
  fieldset.hidden = !isWindows();
  return fieldset;
};

const note = (text: string): HTMLElement =>
  el("span", { class: "setting-note", text });

export const settingsPage = (ctx: Context): Page => {
  const status = el("p", {
    class: "muted",
    id: "settings-status",
    "aria-live": "polite",
  });
  const save = (patch: SettingsPatch): Promise<void> =>
    ctx
      .update(patch)
      .then(() => {
        status.textContent = "Saved.";
      })
      .catch((e: unknown) => notice(failure(e), true));

  const shell = general(ctx, group, note);

  // Switches
  const toggle = (
    id: string,
    label: string,
    text: string,
    field:
      | "hideIrrelevant"
      | "shred"
      | "confirm"
      | "deleteOnReboot"
      | "exitAfterClean",
  ) => {
    const box = el("input", { type: "checkbox", id });
    box.addEventListener("change", () => void save({ [field]: box.checked }));
    const row = el(
      "label",
      { class: "setting", for: id },
      box,
      el(
        "span",
        { class: "setting-text" },
        el("span", { text: label }),
        note(text),
      ),
    );
    return { box, row };
  };
  const hide = toggle(
    "set-hide",
    `Hide cleaners for programs not on this ${machineName()}`,
    "BleachBit's “Hide irrelevant cleaners”.",
    "hideIrrelevant",
  );
  const shred = toggle(
    "set-shred",
    "Overwrite files before deleting them",
    "Slower, but deleted files cannot be recovered. SSDs may keep old copies regardless.",
    "shred",
  );
  const ask = toggle(
    "set-confirm",
    "Ask before cleaning",
    "Shows what will go, warnings and running programs first.",
    "confirm",
  );
  const reboot = toggle(
    "set-reboot",
    "Delete files in use when Windows next starts",
    ctx.info.elevated
      ? "Files a program holds open are queued for deletion at restart."
      : "Needs WattClean to run as administrator.",
    "deleteOnReboot",
  );
  const exit = toggle(
    "set-exit",
    "Exit after cleaning",
    "Closes WattClean as soon as a clean finishes.",
    "exitAfterClean",
  );

  // Keep list and custom paths
  const keep = pathList(ctx, save, "keep-list", "keep", "Nothing is kept yet.");
  const custom = pathList(
    ctx,
    save,
    "custom-list",
    "customPaths",
    "No custom paths yet.",
  );

  // Community cleaners (winapp2.ini)
  const communityText = el("span", {
    id: "community-status",
    class: "setting-note",
  });
  const community = toggleCommunity();
  function toggleCommunity() {
    const box = el("input", { type: "checkbox", id: "set-community" });
    box.addEventListener(
      "change",
      () => void save({ community: box.checked }).then(refreshCommunity),
    );
    return box;
  }
  const refreshCommunity = async () => {
    try {
      const catalog = await api.getCatalog();
      const c = catalog.community;
      communityText.textContent = !c.present
        ? "No winapp2.ini imported."
        : c.enabled
          ? `winapp2.ini adds ${plural(c.cleaners, "cleaner")}.`
          : "winapp2.ini is imported but switched off.";
      removeCommunity.disabled = !c.present;
      problems.replaceChildren(
        ...catalog.problems.map((p) => el("li", { text: p })),
      );
      problems.hidden = catalog.problems.length === 0;
    } catch (error) {
      notice(failure(error), true);
    }
  };
  const busyButton = (
    id: string,
    text: string,
    work: () => Promise<unknown>,
  ) => {
    const button = el("button", {
      type: "button",
      class: "secondary",
      id,
      text,
    });
    button.addEventListener("click", () => {
      button.disabled = true;
      work()
        .then(refreshCommunity)
        .catch((e: unknown) => notice(failure(e), true))
        .finally(() => {
          button.disabled = false;
        });
    });
    return button;
  };
  const importCommunity = busyButton(
    "community-import",
    "Import winapp2.ini…",
    async () => {
      const status = await api.importWinapp2();
      if (status)
        notice(`Imported: ${plural(status.cleaners, "community cleaner")}.`);
    },
  );
  const downloadCommunity = busyButton(
    "community-download",
    "Download the latest",
    async () => {
      const answer = await confirm({
        title: "Download winapp2.ini?",
        message:
          "WattClean fetches the community's current winapp2.ini from GitHub (raw.githubusercontent.com) over HTTPS. Nothing about you is sent. It goes online only when you click this.",
        ok: "Download",
      });
      if (!answer) return;
      const status = await api.downloadWinapp2();
      notice(`Downloaded: ${plural(status.cleaners, "community cleaner")}.`);
    },
  );
  const removeCommunity = busyButton("community-remove", "Remove", () =>
    api.removeWinapp2(),
  );

  // Custom cleaners
  const problems = el("ul", {
    class: "problems",
    id: "cleaner-problems",
    hidden: true,
  });
  const openFolder = busyButton(
    "cleaners-open",
    "Open the cleaners folder",
    () => api.openCleanersFolder(),
  );
  const reload = busyButton("cleaners-reload", "Reload cleaners", async () => {
    await api.reloadCatalog();
    notice("Cleaners reloaded.");
  });

  // Windows only: Linux and macOS delete a file in use at once, and
  // winapp2.ini describes Windows programs.
  reboot.row.hidden = !isWindows();

  const apply = (s: Settings) => {
    hide.box.checked = s.hideIrrelevant;
    shred.box.checked = s.shred;
    ask.box.checked = s.confirm;
    reboot.box.checked = s.deleteOnReboot;
    reboot.box.disabled = !ctx.info.elevated && !s.deleteOnReboot;
    exit.box.checked = s.exitAfterClean;
    community.checked = s.community;
    keep.draw(s.keep);
    custom.draw(s.customPaths);
  };
  ctx.onSettings(apply);

  const element = el(
    "section",
    { class: "page", id: "page-settings" },
    el(
      "div",
      { class: "scroll settings" },
      shell.appearance,
      shell.background,
      group(
        "Disk cleaner",
        "settings-cleaning",
        hide.row,
        shred.row,
        ask.row,
        reboot.row,
        exit.row,
      ),
      group(
        "Keep list",
        "settings-keep",
        note(
          "Files and folders here are never deleted or shredded, whatever a cleaner says (BleachBit's whitelist).",
        ),
        keep.list,
        keep.buttons,
      ),
      group(
        "Custom paths",
        "settings-custom",
        note(
          `Files and folders deleted when ${osName()} › Custom paths is ticked in the Cleaner. Folders go with everything in them.`,
        ),
        custom.list,
        custom.buttons,
      ),
      communityGroup(
        "Community cleaners",
        "settings-community",
        note(
          "winapp2.ini is a list of thousands of extra cleaners kept by the community, which BleachBit and CCleaner also read.",
        ),
        el(
          "label",
          { class: "setting", for: "set-community" },
          community,
          el(
            "span",
            { class: "setting-text" },
            el("span", { text: "Use community cleaners" }),
            communityText,
          ),
        ),
        el(
          "div",
          { class: "row-buttons" },
          importCommunity,
          downloadCommunity,
          removeCommunity,
        ),
      ),
      group(
        "Custom cleaners",
        "settings-cleaners",
        note(
          `Put BleachBit CleanerML files (.xml) or WattClean cleaner files (.toml) in ${ctx.info.cleanersDir}, then reload.`,
        ),
        el("div", { class: "row-buttons" }, openFolder, reload),
        problems,
      ),
      ...ctx.sections.map((section) => {
        section.element.hidden = false;
        section.element.classList.add("module-settings");
        return group(section.title, `settings-${section.id}`, section.element);
      }),
      shell.about,
      status,
    ),
  );
  apply(ctx.settings());
  shell.apply(ctx.shell());
  return {
    element,
    commands: [],
    onShow: () => {
      void refreshCommunity();
      for (const section of ctx.sections) section.onShow?.();
    },
  };
};
