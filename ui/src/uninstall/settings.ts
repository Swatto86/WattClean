/**
 * The uninstaller's settings, shown in WattClean's Settings page (the look
 * is the shell's). Every change is saved at once.
 */
import { api, MODES, type Settings } from "./api.ts";
import { byId } from "./dom.ts";

export function wireSettings(
  settings: Settings,
  changed: (next: Settings) => void,
): void {
  const mode = byId("set-mode", HTMLSelectElement);
  const restore = byId("set-restore", HTMLInputElement);
  const quiet = byId("set-quiet", HTMLInputElement);
  const system = byId("set-system", HTMLInputElement);
  const status = byId("settings-status", HTMLParagraphElement);
  mode.value = settings.mode;
  restore.checked = settings.restorePoint;
  quiet.checked = settings.quiet;
  system.checked = settings.showSystemComponents;
  let current = settings;
  const save = async () => {
    const next: Settings = {
      ...current,
      mode: MODES.find((value) => value === mode.value) ?? current.mode,
      restorePoint: restore.checked,
      quiet: quiet.checked,
      showSystemComponents: system.checked,
    };
    try {
      await api.saveSettings(next);
      current = next;
      status.textContent = "Saved.";
      changed(next);
    } catch (error: unknown) {
      status.textContent = `Not saved: ${error instanceof Error ? error.message : String(error)}`;
    }
  };
  for (const control of [mode, restore, quiet, system]) {
    control.addEventListener("change", () => void save());
  }
}
