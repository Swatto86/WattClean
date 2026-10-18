// Which options are ticked, and how a cleaner's own box reads from them.
// Pure, so tested alone.

export interface OptionLike {
  id: string;
  isDefault: boolean;
  warning: string | null;
}

export interface CleanerLike {
  id: string;
  name: string;
  description: string;
  present: boolean;
  options: OptionLike[];
}

export const key = (cleaner: string, option: string): string =>
  `${cleaner}.${option}`;

export type BoxState = "on" | "off" | "mixed";

/** A cleaner's box: on when every option is ticked, mixed when some are. */
export const cleanerState = (
  cleaner: CleanerLike,
  ticked: ReadonlySet<string>,
): BoxState => {
  const on = cleaner.options.filter((o) =>
    ticked.has(key(cleaner.id, o.id)),
  ).length;
  if (on === 0) return "off";
  return on === cleaner.options.length ? "on" : "mixed";
};

/**
 * Ticking a cleaner's box ticks its options, except those with a warning,
 * which are ticked one by one after reading it. Unticking clears them all.
 */
export const toggleCleaner = (
  cleaner: CleanerLike,
  ticked: ReadonlySet<string>,
): Set<string> => {
  const next = new Set(ticked);
  const state = cleanerState(cleaner, ticked);
  for (const option of cleaner.options) {
    const k = key(cleaner.id, option.id);
    if (state === "off") {
      if (!option.warning) next.add(k);
    } else {
      next.delete(k);
    }
  }
  // A cleaner whose options all carry warnings still gets nothing ticked.
  return next;
};

export const defaults = (cleaners: CleanerLike[]): Set<string> =>
  new Set(
    cleaners.flatMap((c) =>
      c.options.filter((o) => o.isDefault).map((o) => key(c.id, o.id)),
    ),
  );

/** Cleaners matching a search, by name, description or an option's name. */
export const matches = (
  cleaner: CleanerLike,
  query: string,
  optionNames: string[],
): boolean => {
  const q = query.trim().toLowerCase();
  if (!q) return true;
  return (
    cleaner.name.toLowerCase().includes(q) ||
    cleaner.description.toLowerCase().includes(q) ||
    optionNames.some((name) => name.toLowerCase().includes(q))
  );
};

/** Every option of the cleaners that are shown, for Select all. */
export const everything = (
  cleaners: CleanerLike[],
  hideIrrelevant: boolean,
): string[] =>
  cleaners
    .filter((c) => c.present || !hideIrrelevant)
    .flatMap((c) => c.options.map((o) => key(c.id, o.id)));

/** The ticked keys of cleaners that are shown, so hidden ones never run. */
export const runnable = (
  cleaners: CleanerLike[],
  ticked: ReadonlySet<string>,
  hideIrrelevant: boolean,
): string[] =>
  everything(cleaners, hideIrrelevant).filter((k) => ticked.has(k));
