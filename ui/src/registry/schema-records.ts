/** Shapes of the Backup Center, Tuner and Watcher results. */
import { bool, list, num, oneOf, optStr, record, str } from "./decode.ts";
import { pairs, regKey, type RegKey } from "./schema.ts";

export const BACKUP_KINDS = ["clean", "search", "manual", "full"] as const;

export interface Summary {
  id: string;
  created: number;
  kind: (typeof BACKUP_KINDS)[number];
  label: string;
  items: number;
  removed: number;
  restored: number;
  failed: number;
  damaged: string | null;
}

export function summary(v: unknown, w: string): Summary {
  const r = record(v, w);
  return {
    id: str(r["id"], `${w}.id`),
    created: num(r["created"], `${w}.created`),
    kind: oneOf(r["kind"], `${w}.kind`, BACKUP_KINDS),
    label: str(r["label"], `${w}.label`),
    items: num(r["items"], `${w}.items`),
    removed: num(r["removed"], `${w}.removed`),
    restored: num(r["restored"], `${w}.restored`),
    failed: num(r["failed"], `${w}.failed`),
    damaged: optStr(r["damaged"], `${w}.damaged`),
  };
}

export const ITEM_STATES = [
  "pending",
  "removed",
  "kept",
  "restored",
  "failed",
] as const;

export interface ItemView {
  index: number;
  entry: string;
  detail: string;
  category: string | null;
  state: (typeof ITEM_STATES)[number];
  reason: string | null;
  restorable: boolean;
}

export function itemView(v: unknown, w: string): ItemView {
  const r = record(v, w);
  return {
    index: num(r["index"], `${w}.index`),
    entry: str(r["entry"], `${w}.entry`),
    detail: str(r["detail"], `${w}.detail`),
    category: optStr(r["category"], `${w}.category`),
    state: oneOf(r["state"], `${w}.state`, ITEM_STATES),
    reason: optStr(r["reason"], `${w}.reason`),
    restorable: bool(r["restorable"], `${w}.restorable`),
  };
}

export interface RestoreOutcome {
  restored: number;
  skipped: [string, string][];
}

export function restoreOutcome(v: unknown, w: string): RestoreOutcome {
  const r = record(v, w);
  return {
    restored: num(r["restored"], `${w}.restored`),
    skipped: pairs(r["skipped"], `${w}.skipped`),
  };
}

export const GROUPS = [
  "appearance",
  "explorer",
  "taskbar",
  "performance",
  "privacy",
  "security",
  "updates",
] as const;
export const NEEDS = ["nothing", "explorer", "signOut", "restart"] as const;

export interface Tweak {
  id: string;
  group: (typeof GROUPS)[number];
  title: string;
  description: string;
  needs: (typeof NEEDS)[number];
  machine: boolean;
  on: boolean;
  changedByApp: boolean;
  unavailable: string | null;
}

export function tweak(v: unknown, w: string): Tweak {
  const r = record(v, w);
  return {
    id: str(r["id"], `${w}.id`),
    group: oneOf(r["group"], `${w}.group`, GROUPS),
    title: str(r["title"], `${w}.title`),
    description: str(r["description"], `${w}.description`),
    needs: oneOf(r["needs"], `${w}.needs`, NEEDS),
    machine: bool(r["machine"], `${w}.machine`),
    on: bool(r["on"], `${w}.on`),
    changedByApp: bool(r["changedByApp"], `${w}.changedByApp`),
    unavailable: optStr(r["unavailable"], `${w}.unavailable`),
  };
}

export interface ResetOutcome {
  reset: number;
  failed: [string, string][];
}

export function resetOutcome(v: unknown, w: string): ResetOutcome {
  const r = record(v, w);
  return {
    reset: num(r["reset"], `${w}.reset`),
    failed: pairs(r["failed"], `${w}.failed`),
  };
}

export interface WatchView {
  key: RegKey;
  paused: boolean;
  events: number;
  problem: string | null;
}

export function watchView(v: unknown, w: string): WatchView {
  const r = record(v, w);
  return {
    key: regKey(r["key"], `${w}.key`),
    paused: bool(r["paused"], `${w}.paused`),
    events: num(r["events"], `${w}.events`),
    problem: optStr(r["problem"], `${w}.problem`),
  };
}

export const OPS = [
  "keyCreated",
  "keyDeleted",
  "valueAdded",
  "valueChanged",
  "valueDeleted",
  "valueSet",
] as const;

/** The process that made a change, when event tracing saw it. */
export interface Actor {
  pid: number;
  process: string;
}

export interface WatchEvent {
  seq: number;
  time: number;
  op: (typeof OPS)[number];
  key: RegKey;
  value: string | null;
  kind: string | null;
  old: string | null;
  new: string | null;
  actor: Actor | null;
}

function actor(v: unknown, w: string): Actor | null {
  if (v === null || v === undefined) return null;
  const r = record(v, w);
  return {
    pid: num(r["pid"], `${w}.pid`),
    process: str(r["process"], `${w}.process`),
  };
}

export function watchEvent(v: unknown, w: string): WatchEvent {
  const r = record(v, w);
  return {
    seq: num(r["seq"], `${w}.seq`),
    time: num(r["time"], `${w}.time`),
    op: oneOf(r["op"], `${w}.op`, OPS),
    key: regKey(r["key"], `${w}.key`),
    value: optStr(r["value"], `${w}.value`),
    kind: optStr(r["kind"], `${w}.kind`),
    old: optStr(r["old"], `${w}.old`),
    new: optStr(r["new"], `${w}.new`),
    actor: actor(r["actor"], `${w}.actor`),
  };
}

export const TRACE_STATES = ["on", "idle", "needsAdmin", "failed"] as const;

/** Whether the watcher can name the process behind each change. */
export interface TraceState {
  state: (typeof TRACE_STATES)[number];
  problem: string | null;
}

export function traceState(v: unknown, w: string): TraceState {
  const r = record(v, w);
  return {
    state: oneOf(r["state"], `${w}.state`, TRACE_STATES),
    problem: optStr(r["problem"], `${w}.problem`),
  };
}

export function events(v: unknown, w: string): WatchEvent[] {
  return list(v, w, watchEvent);
}
