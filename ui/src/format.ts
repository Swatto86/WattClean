// Numbers, sizes and dates as the window shows them. Pure, so tested alone.

const UNITS = ["bytes", "KB", "MB", "GB", "TB"] as const;

/** 0 bytes, 999 bytes, 1.0 KB, 12.3 MB, 1.21 GB (powers of 1024, as Explorer). */
export const bytes = (value: number): string => {
  if (!Number.isFinite(value) || value < 0) return "0 bytes";
  if (value < 1024)
    return `${Math.round(value)} ${value === 1 ? "byte" : "bytes"}`;
  let size = value;
  let unit = 0;
  while (size >= 1024 && unit < UNITS.length - 1) {
    size /= 1024;
    unit += 1;
  }
  const digits = size >= 100 ? 0 : size >= 10 ? 1 : 2;
  return `${size.toFixed(digits)} ${UNITS[unit]}`;
};

/** 1,234 */
export const count = (value: number): string =>
  Math.round(value).toLocaleString("en-GB");

/** "1 file", "3 files". */
export const plural = (value: number, one: string, many = `${one}s`): string =>
  `${count(value)} ${value === 1 ? one : many}`;

/** A date and time in the reader's own format. */
export const when = (secondsSince1970: number): string =>
  new Date(secondsSince1970 * 1000).toLocaleString(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  });

/** 850 ms, 4.2 s, 2 min 5 s */
export const duration = (ms: number): string => {
  if (ms < 1000) return `${Math.round(ms)} ms`;
  const seconds = ms / 1000;
  if (seconds < 60) return `${seconds.toFixed(1)} s`;
  const minutes = Math.floor(seconds / 60);
  return `${minutes} min ${Math.round(seconds - minutes * 60)} s`;
};

/** A share 0–100 for a progress bar, safe when the total is unknown. */
export const percent = (done: number, total: number): number =>
  total > 0 ? Math.max(0, Math.min(100, (done / total) * 100)) : 0;
