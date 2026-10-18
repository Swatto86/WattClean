/** Display formatting, kept pure so it can be tested without a page. */
import type { Counts, RegPath, Target } from "./api.ts";

export function bytes(count: number): string {
  if (count < 1024) return `${count} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let value = count / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value >= 100 ? value.toFixed(0) : value.toFixed(1)} ${units[unit]}`;
}

export function kilobytes(kb: number | null): string {
  return kb === null ? "" : bytes(kb * 1024);
}

/** Seconds since 1970 as a local date and time, e.g. "14 Oct 2026, 15:04". */
export function when(seconds: number): string {
  return new Date(seconds * 1000).toLocaleString("en-GB", {
    day: "numeric",
    month: "short",
    year: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function regPath(key: RegPath): string {
  return `${key.hive}\\${key.path}`;
}

export function target(item: Target): string {
  switch (item.kind) {
    case "key":
      return regPath(item.key);
    case "value":
      return `${regPath(item.key)} → ${item.name === "" ? "(Default)" : item.name}`;
    case "folder":
    case "file":
      return item.path;
  }
}

export function plural(count: number, one: string, many = `${one}s`): string {
  return `${count} ${count === 1 ? one : many}`;
}

export function countsText(counts: Counts): string {
  const parts = [
    counts.keys > 0 ? plural(counts.keys, "registry key") : "",
    counts.values > 0 ? plural(counts.values, "registry value") : "",
    counts.folders > 0 ? plural(counts.folders, "folder") : "",
    counts.files > 0 ? plural(counts.files, "file") : "",
  ].filter((part) => part !== "");
  return parts.length === 0 ? "nothing" : parts.join(", ");
}

/** A stable colour for a program's initial, so rows are easy to scan. */
export function avatarColour(name: string): string {
  let hash = 0;
  for (const char of name) hash = (hash * 31 + char.charCodeAt(0)) >>> 0;
  return `hsl(${hash % 360} 55% 45%)`;
}

export function initial(name: string): string {
  const match = /[\p{L}\p{N}]/u.exec(name);
  return match ? match[0].toUpperCase() : "?";
}
