/** The install-log shapes Rust commands return, and their checks. */
import { list, num, oneOf, optStr, record, str } from "./decode.ts";
import { target, type Target } from "./schema.ts";

export interface LogSummary {
  id: string;
  created: number;
  installer: string;
  program: string | null;
  programId: string | null;
  createdItems: number;
  changedItems: number;
  eventsLost: number;
}

export interface LogItem {
  target: Target;
  recorded: "created" | "changed";
}

export interface InstallLog {
  id: string;
  program: string | null;
  installer: string;
  items: LogItem[];
  eventsLost: number;
  unnamed: number;
}

export const logSummary = (v: unknown, w: string): LogSummary => {
  const o = record(v, w);
  const n = (name: string) => num(o[name], `${w}.${name}`);
  return {
    id: str(o["id"], `${w}.id`),
    created: n("created"),
    installer: str(o["installer"], `${w}.installer`),
    program: optStr(o["program"], `${w}.program`),
    programId: optStr(o["programId"], `${w}.programId`),
    createdItems: n("createdItems"),
    changedItems: n("changedItems"),
    eventsLost: n("eventsLost"),
  };
};

export const installLog = (v: unknown, w: string): InstallLog => {
  const o = record(v, w);
  const item = (entry: unknown, at: string): LogItem => {
    const fields = record(entry, at);
    return {
      target: target(fields["target"], `${at}.target`),
      recorded: oneOf(fields["recorded"], `${at}.recorded`, [
        "created",
        "changed",
      ] as const),
    };
  };
  return {
    id: str(o["id"], `${w}.id`),
    program: optStr(o["program"], `${w}.program`),
    installer: str(o["installer"], `${w}.installer`),
    items: list(o["items"], `${w}.items`, item),
    eventsLost: num(o["eventsLost"], `${w}.eventsLost`),
    unnamed: num(o["unnamed"], `${w}.unnamed`),
  };
};

export const traceStarted = (v: unknown, w: string) => {
  const o = record(v, w);
  return {
    id: num(o["id"], `${w}.id`),
    installer: str(o["installer"], `${w}.installer`),
  };
};
