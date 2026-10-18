// Narrowing data that crosses the IPC boundary. Anything that does not
// have the expected shape throws, so a mismatch shows as an error instead
// of `undefined` creeping through the page.

export class DecodeError extends Error {}

export type Fields = Record<string, unknown>;

const isFields = (value: unknown): value is Fields =>
  typeof value === "object" && value !== null && !Array.isArray(value);

export const obj = (value: unknown, what: string): Fields => {
  if (!isFields(value)) throw new DecodeError(`${what} is not an object`);
  return value;
};

export const str = (value: unknown, what: string): string => {
  if (typeof value !== "string") throw new DecodeError(`${what} is not text`);
  return value;
};

export const num = (value: unknown, what: string): number => {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    throw new DecodeError(`${what} is not a number`);
  }
  return value;
};

export const bool = (value: unknown, what: string): boolean => {
  if (typeof value !== "boolean")
    throw new DecodeError(`${what} is not true or false`);
  return value;
};

export const optStr = (value: unknown, what: string): string | null =>
  value === null || value === undefined ? null : str(value, what);

export const arr = <T>(
  value: unknown,
  what: string,
  each: (item: unknown, what: string) => T,
): T[] => {
  if (!Array.isArray(value)) throw new DecodeError(`${what} is not a list`);
  return value.map((item, index) => each(item, `${what}[${index}]`));
};

export const oneOf = <T extends string>(
  value: unknown,
  what: string,
  allowed: readonly T[],
): T => {
  const text = str(value, what);
  const found = allowed.find((option) => option === text);
  if (found === undefined) {
    throw new DecodeError(`${what} is not one of ${allowed.join(", ")}`);
  }
  return found;
};

/** The sentence a failed command sent, whatever shape the failure took. */
export const failure = (error: unknown): string => {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  if (typeof error === "object" && error !== null && "message" in error) {
    const message = error.message;
    if (typeof message === "string") return message;
  }
  return "Something went wrong.";
};
