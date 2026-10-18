/** Filtering and sorting the program list: pure, so it is unit-tested. */
import type { Program } from "./api.ts";

export type SortKey = "name" | "publisher" | "version" | "size" | "date";
export interface Sort {
  key: SortKey;
  ascending: boolean;
}

export function matches(program: Program, query: string): boolean {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  const haystack = `${program.name} ${program.publisher ?? ""}`.toLowerCase();
  return words.every((word) => haystack.includes(word));
}

const collator = new Intl.Collator("en-GB", {
  numeric: true,
  sensitivity: "base",
});

function compare(a: Program, b: Program, key: SortKey): number {
  switch (key) {
    case "name":
      return collator.compare(a.name, b.name);
    case "publisher":
      return collator.compare(a.publisher ?? "", b.publisher ?? "");
    case "version":
      return collator.compare(a.version ?? "", b.version ?? "");
    case "size":
      return (a.sizeKb ?? -1) - (b.sizeKb ?? -1);
    case "date":
      return (a.installDate ?? "").localeCompare(b.installDate ?? "");
  }
}

/** Programs matching `query`, ordered by `sort`; ties fall back to name. */
export function view(
  programs: readonly Program[],
  query: string,
  sort: Sort,
): Program[] {
  const direction = sort.ascending ? 1 : -1;
  return programs
    .filter((program) => matches(program, query))
    .sort(
      (a, b) =>
        direction * compare(a, b, sort.key) || collator.compare(a.name, b.name),
    );
}

/** The index to select after an arrow key, clamped to the list. */
export function step(index: number, length: number, key: string): number {
  if (length === 0) return -1;
  const next =
    key === "ArrowDown"
      ? index + 1
      : key === "ArrowUp"
        ? index - 1
        : key === "Home"
          ? 0
          : key === "End"
            ? length - 1
            : key === "PageDown"
              ? index + 10
              : key === "PageUp"
                ? index - 10
                : index;
  return Math.max(0, Math.min(length - 1, next));
}
