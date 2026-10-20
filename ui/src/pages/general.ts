// The shell's own settings, at the top and bottom of Settings: the look
// (theme, accent, text size), running in the background (the tray, starting
// with Windows, quitting), and about (with macOS's Full Disk Access). Each
// change saves at once.

import { api, TEXT_SIZES, THEMES, type ShellSettings } from "../api.ts";
import type { Context } from "../context.ts";
import { el, notice } from "../dom.ts";
import {
  isWindows,
  noticesPlace,
  osName,
  platform,
  trayName,
} from "../platform.ts";
import { ACCENTS } from "../theme.ts";

type Group = (
  title: string,
  id: string,
  ...children: (Node | null)[]
) => HTMLFieldSetElement;

export interface General {
  appearance: HTMLFieldSetElement;
  background: HTMLFieldSetElement;
  about: HTMLFieldSetElement;
  /** Shows the shell's settings as saved now. */
  apply: (shell: ShellSettings) => void;
}

const select = (
  id: string,
  label: string,
  options: [string, string][],
): HTMLSelectElement =>
  el(
    "select",
    { id, "aria-label": label },
    ...options.map(([value, text]) => el("option", { value, text })),
  );

export const general = (
  ctx: Context,
  group: Group,
  note: (text: string) => HTMLElement,
): General => {
  const fail = (e: unknown) => notice(String(e), true);

  const theme = select("set-theme", "Theme", [
    ["system", `Same as ${osName()}`],
    ["light", "Light"],
    ["dark", "Dark"],
  ]);
  theme.addEventListener("change", () => {
    const value = THEMES.find((t) => t === theme.value);
    if (value) ctx.updateShell({ theme: value }).then(apply, fail);
  });
  const size = select("set-text-size", "Text size", [
    ["small", "Small"],
    ["normal", "Normal"],
    ["large", "Large"],
  ]);
  size.addEventListener("change", () => {
    const value = TEXT_SIZES.find((t) => t === size.value);
    if (value) ctx.updateShell({ textSize: value }).then(apply, fail);
  });
  const swatches = el("div", {
    class: "swatches",
    role: "group",
    "aria-label": "Accent colour",
  });
  for (const [id, accent] of Object.entries(ACCENTS)) {
    const swatch = el("button", {
      type: "button",
      class: "swatch",
      title: accent.name,
      "aria-label": accent.name,
      "data-accent": id,
    });
    swatch.style.setProperty("--swatch", accent.dark);
    swatch.addEventListener("click", () => {
      ctx.updateShell({ accent: id }).then(apply, fail);
    });
    swatches.append(swatch);
  }

  const tray = el("input", { type: "checkbox", id: "set-tray" });
  tray.addEventListener("change", () => {
    ctx.updateShell({ keepInTray: tray.checked }).then(apply, fail);
  });
  const quit = el("button", {
    type: "button",
    class: "secondary",
    id: "quit-app",
    text: "Quit WattClean",
  });
  quit.addEventListener("click", () => {
    api.quitApp().catch(fail);
  });
  const start = ctx.startWithWindows;
  if (start) start.classList.add("setting-embed");

  const restart = el("button", {
    type: "button",
    class: "secondary",
    text: "Restart as administrator",
    // Linux and macOS ask for administrator rights per task instead.
    hidden: ctx.info.elevated || !isWindows(),
  });
  restart.addEventListener("click", () => {
    api.restartAsAdmin().catch(fail);
  });
  const openData = el("button", {
    type: "button",
    class: "secondary",
    id: "open-data-folder",
    text: "Open the data folder",
  });
  openData.addEventListener("click", () => {
    api.openDataFolder().catch(fail);
  });

  // macOS keeps Safari, Mail and other apps' data from WattClean until it
  // is given Full Disk Access.
  const mac = platform() === "macos";
  const access = el("p", {
    id: "privacy-access",
    hidden: !mac,
    text: "Checking Full Disk Access…",
  });
  const privacy = el("button", {
    type: "button",
    class: "secondary",
    id: "open-privacy",
    text: "Open Privacy & Security",
    hidden: !mac,
  });
  privacy.addEventListener("click", () => {
    api.openPrivacySettings().catch(fail);
  });
  if (mac) {
    api.privacyAccess().then((given) => {
      access.textContent =
        given === null
          ? "WattClean could not tell whether it has Full Disk Access."
          : given
            ? "WattClean has Full Disk Access."
            : "WattClean does not have Full Disk Access, so what macOS protects (Safari, Mail, other apps' data) is not cleaned. Once it is given, macOS asks to quit and reopen WattClean.";
    }, fail);
  }

  const apply = (shell: ShellSettings) => {
    theme.value = shell.theme;
    size.value = shell.textSize;
    tray.checked = shell.keepInTray;
    for (const swatch of swatches.querySelectorAll<HTMLButtonElement>(
      ".swatch",
    )) {
      swatch.setAttribute(
        "aria-pressed",
        String(swatch.dataset["accent"] === shell.accent),
      );
    }
  };

  const row = (label: string, control: HTMLElement) =>
    el("div", { class: "setting" }, el("span", { text: label }), control);

  return {
    appearance: group(
      "Appearance",
      "settings-appearance",
      row("Theme", theme),
      row("Accent colour", swatches),
      row("Text size", size),
    ),
    background: group(
      "Running in the background",
      "settings-background",
      el(
        "label",
        { class: "setting", for: "set-tray" },
        tray,
        el(
          "span",
          { class: "setting-text" },
          el("span", { text: `Keep running in the ${trayName()}` }),
          note(
            !ctx.info.trayShown
              ? `This desktop shows no ${trayName()}, so closing the window quits WattClean.`
              : isWindows()
                ? "Closing the window hides it to the icon by the clock, so the Registry Watcher keeps watching. Quit from the icon's menu or below."
                : `Closing the window hides it to the icon in the ${trayName()}. Quit from the icon's menu or below.`,
          ),
        ),
      ),
      start,
      el("div", { class: "row-buttons" }, quit),
    ),
    about: group(
      "About",
      "settings-about",
      el("p", {
        text: `WattClean ${ctx.info.version}, running as ${ctx.info.elevated ? "administrator" : "a standard user"}.`,
      }),
      note(
        `Settings, history and backups are kept in ${ctx.info.dataDir}, one folder per tool.`,
      ),
      note(
        `WattClean is MIT-licensed. The licences of the open-source packages it is built with are in THIRD-PARTY-NOTICES.txt ${noticesPlace()}.`,
      ),
      access,
      el("div", { class: "row-buttons" }, openData, privacy, restart),
    ),
    apply,
  };
};
