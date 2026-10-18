// What every page is given by the shell, and what it gives back.

import type {
  AppInfo,
  Settings,
  SettingsPatch,
  ShellPatch,
  ShellSettings,
} from "./api.ts";

/** A module's own settings, shown in a group of the Settings page. */
export interface SettingsSection {
  id: string;
  title: string;
  element: HTMLElement;
  onShow?: () => void;
}

export interface Context {
  info: AppInfo;
  /** The disk cleaner's settings as last saved. */
  settings: () => Settings;
  /** Saves a change to the disk cleaner's settings and tells every page. */
  update: (patch: SettingsPatch) => Promise<Settings>;
  onSettings: (listener: (settings: Settings) => void) => void;
  /** The shell's settings (look, navigation, tray) as last saved. */
  shell: () => ShellSettings;
  updateShell: (patch: ShellPatch) => Promise<ShellSettings>;
  /** The registry cleaner's and uninstaller's settings, for Settings. */
  sections: SettingsSection[];
  /** The registry cleaner's start-with-Windows switch, for Settings. */
  startWithWindows: HTMLElement | null;
  setSelectionStatus: (text: string) => void;
  /** Opens another page. */
  go: (page: string) => void;
}

export interface Page {
  element: HTMLElement;
  /** Buttons for the header's toolbar. */
  commands: HTMLElement[];
  /** Called each time the page is opened. */
  onShow?: () => void;
  /** Present when the page has a search box. */
  onSearch?: (query: string) => void;
}
