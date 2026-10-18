/**
 * Narrowing for data that crosses the IPC boundary. Every command result is
 * `unknown` until one of these has checked its shape.
 */

export class DecodeError extends Error {
  override name = "DecodeError";
}

export type Fields = Record<string, unknown>;

export function record(value: unknown, what: string): Fields {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new DecodeError(`${what} is not an object`);
  }
  return Object.fromEntries(Object.entries(value));
}

export function str(value: unknown, what: string): string {
  if (typeof value !== "string") throw new DecodeError(`${what} is not text`);
  return value;
}

export function num(value: unknown, what: string): number {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    throw new DecodeError(`${what} is not a number`);
  }
  return value;
}

export function bool(value: unknown, what: string): boolean {
  if (typeof value !== "boolean")
    throw new DecodeError(`${what} is not true or false`);
  return value;
}

export function optStr(value: unknown, what: string): string | null {
  return value === null || value === undefined ? null : str(value, what);
}

export function optNum(value: unknown, what: string): number | null {
  return value === null || value === undefined ? null : num(value, what);
}

export function list<T>(
  value: unknown,
  what: string,
  item: (v: unknown, w: string) => T,
): T[] {
  if (!Array.isArray(value)) throw new DecodeError(`${what} is not a list`);
  return value.map((entry: unknown, index) => item(entry, `${what}[${index}]`));
}

export function oneOf<T extends string>(
  value: unknown,
  what: string,
  allowed: readonly T[],
): T {
  const text = str(value, what);
  const found = allowed.find((option) => option === text);
  if (found === undefined)
    throw new DecodeError(`${what} is not one of ${allowed.join(", ")}`);
  return found;
}
