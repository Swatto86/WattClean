/** Shapes of schedules and background running. */
import {
  bool,
  list,
  num,
  oneOf,
  optStr,
  record,
  str,
  type Fields,
} from "./decode.ts";
import { CATEGORIES, MODES, type Category, type Mode } from "./schema.ts";

export type Work =
  | { kind: "clean"; mode: Mode; categories: Category[]; allAccounts: boolean }
  | { kind: "backup"; key: string }
  | { kind: "fullBackup" };

export type Frequency =
  | { every: "day" }
  | { every: "week"; weekday: number }
  | { every: "month"; day: number };

export type Schedule = Frequency & { time: string };

export interface LastRun {
  time: number;
  ok: boolean;
  summary: string;
  backup: string | null;
}

export interface Job {
  id: string;
  name: string;
  work: Work;
  schedule: Schedule;
  enabled: boolean;
  scheduled: boolean;
  problem: string | null;
  last: LastRun | null;
}

/** A job as the editor sends it; `id` is null for a new one. */
export interface JobInput {
  id: string | null;
  name: string;
  work: Work;
  schedule: Schedule;
  enabled: boolean;
}

function work(v: unknown, w: string): Work {
  const r = record(v, w);
  const kind = oneOf(r["kind"], `${w}.kind`, [
    "clean",
    "backup",
    "fullBackup",
  ] as const);
  if (kind === "backup") return { kind, key: str(r["key"], `${w}.key`) };
  if (kind === "fullBackup") return { kind };
  return {
    kind,
    mode: oneOf(r["mode"], `${w}.mode`, MODES),
    categories: list(r["categories"], `${w}.categories`, (c, cw) =>
      oneOf(c, cw, CATEGORIES),
    ),
    allAccounts: bool(r["allAccounts"], `${w}.allAccounts`),
  };
}

function schedule(v: unknown, w: string): Schedule {
  const r: Fields = record(v, w);
  const time = str(r["time"], `${w}.time`);
  const every = oneOf(r["every"], `${w}.every`, [
    "day",
    "week",
    "month",
  ] as const);
  if (every === "week")
    return { every, weekday: num(r["weekday"], `${w}.weekday`), time };
  if (every === "month") return { every, day: num(r["day"], `${w}.day`), time };
  return { every, time };
}

export function lastRun(v: unknown, w: string): LastRun {
  const r = record(v, w);
  return {
    time: num(r["time"], `${w}.time`),
    ok: bool(r["ok"], `${w}.ok`),
    summary: str(r["summary"], `${w}.summary`),
    backup: optStr(r["backup"], `${w}.backup`),
  };
}

export function job(v: unknown, w: string): Job {
  const r = record(v, w);
  return {
    id: str(r["id"], `${w}.id`),
    name: str(r["name"], `${w}.name`),
    work: work(r["work"], `${w}.work`),
    schedule: schedule(r["schedule"], `${w}.schedule`),
    enabled: bool(r["enabled"], `${w}.enabled`),
    scheduled: bool(r["scheduled"], `${w}.scheduled`),
    problem: optStr(r["problem"], `${w}.problem`),
    last:
      r["last"] === null || r["last"] === undefined
        ? null
        : lastRun(r["last"], `${w}.last`),
  };
}

export interface Background {
  startWithWindows: boolean;
  /** Through the sign-in task, with administrator rights. */
  startsElevated: boolean;
  startProblem: string | null;
}

export function background(v: unknown, w: string): Background {
  const r = record(v, w);
  return {
    startWithWindows: bool(r["startWithWindows"], `${w}.startWithWindows`),
    startsElevated: bool(r["startsElevated"], `${w}.startsElevated`),
    startProblem: optStr(r["startProblem"], `${w}.startProblem`),
  };
}
