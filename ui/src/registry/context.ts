/** What every view shares: who is running the app, settings, and names. */
import type { AppInfo, Category, CategoryInfo, Settings } from "./api.ts";

export interface AppContext {
  info: AppInfo;
  titles: Map<Category, CategoryInfo>;
  /** The settings as last saved. */
  settings: () => Settings;
  /** Change, save and apply the settings; throws when they are refused. */
  update: (change: (current: Settings) => Settings) => Promise<void>;
  restartAsAdmin: () => void;
  /** Called after a clean or restore, so the backup list is current. */
  backupsChanged: () => void;
}

/** Add a key to the exclude list, once. */
export async function exclude(context: AppContext, key: string): Promise<void> {
  await context.update((current) =>
    current.excludes.some(
      (entry) => entry.key.toLowerCase() === key.toLowerCase(),
    )
      ? current
      : {
          ...current,
          excludes: [
            ...current.excludes,
            { key, subkeys: false, enabled: true },
          ],
        },
  );
}
