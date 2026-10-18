/** Small formatting helpers shared by the views. */

/** "1 entry", "3 entries". */
export function count(n: number, one: string, many: string): string {
  return `${n.toLocaleString("en-GB")} ${n === 1 ? one : many}`;
}

/** A backup's date from Unix seconds, e.g. "16 Oct 2026, 14:05". */
export function dateTime(seconds: number): string {
  return new Date(seconds * 1000).toLocaleString("en-GB", {
    day: "numeric",
    month: "short",
    year: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

/** A logged change's time from Unix milliseconds, e.g. "14:05:09.123". */
export function timeOfDay(millis: number): string {
  const time = new Date(millis);
  const pad = (n: number, width = 2) => String(n).padStart(width, "0");
  return `${pad(time.getHours())}:${pad(time.getMinutes())}:${pad(time.getSeconds())}.${pad(time.getMilliseconds(), 3)}`;
}

/** Put text on the clipboard. */
export async function copyText(text: string): Promise<void> {
  await navigator.clipboard.writeText(text);
}
