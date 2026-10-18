/** The Schedules view: Auto Clean and Auto Backup jobs. */
import {
  api,
  message,
  type Job,
  type JobInput,
  type Mode,
  type Schedule,
} from "./api.ts";
import type { AppContext } from "./context.ts";
import { confirm, inform, Modal } from "./dialog.ts";
import { byId, h } from "./dom.ts";
import { dateTime } from "./format.ts";
import { item, onMenu, popup, separator } from "./menu.ts";

const WEEKDAYS = [
  "Monday",
  "Tuesday",
  "Wednesday",
  "Thursday",
  "Friday",
  "Saturday",
  "Sunday",
];
const MODE_TEXT: Record<Mode, string> = {
  safe: "Safe",
  moderate: "Moderate",
  advanced: "Advanced",
  custom: "Custom",
};

export function whenText(schedule: Schedule): string {
  if (schedule.every === "week")
    return `Every ${WEEKDAYS[schedule.weekday] ?? "week"} at ${schedule.time}`;
  if (schedule.every === "month")
    return `Day ${schedule.day} of every month at ${schedule.time}`;
  return `Every day at ${schedule.time}`;
}

function whatText(job: Job): string {
  if (job.work.kind === "backup") return `Back up ${job.work.key}`;
  if (job.work.kind === "fullBackup") return "Back up the whole registry";
  const accounts = job.work.allAccounts ? ", every account" : "";
  return `${MODE_TEXT[job.work.mode]} clean${accounts}`;
}

export interface SchedulesView {
  load: () => Promise<void>;
}

export function wireSchedules(context: AppContext): SchedulesView {
  const listing = byId("job-list", HTMLDivElement);
  let jobs: Job[] = [];

  const run = (work: () => Promise<unknown>) =>
    void work()
      .catch((error: unknown) => inform("That did not work", message(error)))
      .then(() => load());

  const runNow = async (job: Job) => {
    const last = await api.runJobNow(job.id);
    context.backupsChanged();
    await inform(`"${job.name}" ran`, last.summary);
  };
  const remove = async (job: Job) => {
    const yes = await confirm(
      "Delete schedule",
      `Delete "${job.name}"? Windows stops running it.`,
      "Delete",
      "danger",
    );
    if (yes) await api.deleteJob(job.id);
  };
  const toggle = (job: Job) => api.setJobEnabled(job.id, !job.enabled);

  const card = (job: Job): HTMLElement => {
    const button = (label: string, work: () => Promise<unknown>) => {
      const element = h("button", { type: "button", class: "ghost" }, label);
      element.addEventListener("click", () => run(work));
      return element;
    };
    const status = !job.enabled
      ? h("span", { class: "chip" }, "Paused")
      : job.scheduled
        ? h("span", { class: "chip ok" }, "Scheduled")
        : h("span", { class: "chip admin" }, "Not scheduled");
    return h(
      "article",
      { class: "card entry", "data-id": job.id, tabindex: "0" },
      h("div", {}, h("b", {}, job.name), " ", status),
      h(
        "div",
        { class: "actions" },
        button("Run now", () => runNow(job)),
        button("Edit…", () => edit(job)),
        button(job.enabled ? "Pause" : "Resume", () => toggle(job)),
        button("Delete", () => remove(job)),
      ),
      h(
        "div",
        { class: "meta" },
        `${whatText(job)} · ${whenText(job.schedule)}`,
        job.last
          ? h(
              "div",
              { class: job.last.ok ? "" : "error" },
              `Last run ${dateTime(job.last.time)}: ${job.last.summary}`,
            )
          : null,
        job.problem ? h("div", { class: "notice" }, job.problem) : null,
      ),
    );
  };

  const load = async () => {
    jobs = await api.listJobs();
    listing.replaceChildren(
      ...(jobs.length > 0
        ? jobs.map(card)
        : [
            h(
              "p",
              { class: "empty" },
              "No schedules yet. New schedule… sets up an automatic clean or backup.",
            ),
          ]),
    );
  };

  const edit = async (job: Job | null) => {
    const saved = await editor(job);
    if (saved?.problem) await inform("Saved, but not scheduled", saved.problem);
  };

  byId("job-new", HTMLButtonElement).addEventListener("click", () =>
    run(() => edit(null)),
  );
  onMenu("jobs", (target, at) => {
    const id = target.closest<HTMLElement>("[data-id]")?.dataset["id"];
    const job = jobs.find((j) => j.id === id);
    if (!job) return;
    run(async () => {
      const chosen = await popup(
        [
          item("run", "&Run now"),
          item("edit", "&Edit…"),
          item("toggle", job.enabled ? "&Pause" : "Re&sume"),
          separator,
          item("delete", "&Delete"),
        ],
        at,
      );
      if (chosen === "run") await runNow(job);
      if (chosen === "edit") await edit(job);
      if (chosen === "toggle") await toggle(job);
      if (chosen === "delete") await remove(job);
    });
  });
  return { load };
}

/** The schedule editor: saves on Save, keeping the editor open with the
 * reason when the schedule is refused; resolves with the saved job. */
function editor(job: Job | null): Promise<Job | null> {
  return new Promise((resolve) => {
    const modal = new Modal(job ? "Edit schedule" : "New schedule");
    const select = (id: string, options: [string, string][], value: string) => {
      const element = h("select", { id });
      for (const [key, label] of options) {
        element.append(h("option", { value: key }, label));
      }
      element.value = value;
      return element;
    };
    const work = job?.work;
    const schedule = job?.schedule;
    const name = h("input", { id: "job-name", maxlength: "80" });
    name.value = job?.name ?? "Weekly clean";
    const kind = select(
      "job-kind",
      [
        ["clean", "Clean the registry"],
        ["backup", "Back up a key"],
        ["fullBackup", "Back up the whole registry (administrator)"],
      ],
      work?.kind ?? "clean",
    );
    const mode = select(
      "job-mode",
      [
        ["safe", "Safe"],
        ["moderate", "Moderate"],
        ["advanced", "Advanced"],
      ],
      work?.kind === "clean" ? work.mode : "safe",
    );
    const all = h("input", { type: "checkbox", id: "job-all" });
    all.checked = work?.kind === "clean" && work.allAccounts;
    const key = h("input", {
      id: "job-key",
      maxlength: "1000",
      placeholder: "HKEY_CURRENT_USER\\Software\\Example",
    });
    key.value = work?.kind === "backup" ? work.key : "";
    const every = select(
      "job-every",
      [
        ["day", "Every day"],
        ["week", "Every week"],
        ["month", "Every month"],
      ],
      schedule?.every ?? "week",
    );
    const weekday = select(
      "job-weekday",
      WEEKDAYS.map((day, at) => [String(at), day]),
      schedule?.every === "week" ? String(schedule.weekday) : "0",
    );
    const day = h("input", {
      type: "number",
      id: "job-day",
      min: "1",
      max: "28",
    });
    day.value = schedule?.every === "month" ? String(schedule.day) : "1";
    const time = h("input", { type: "time", id: "job-time" });
    time.value = schedule?.time ?? "03:00";
    const cleanPart = h(
      "div",
      { class: "field" },
      h("label", { class: "field" }, "Scan mode", mode),
      h("label", { class: "check" }, all, "Every account on this PC"),
    );
    const keyPart = h(
      "label",
      { class: "field" },
      "Key, with everything beneath it",
      key,
    );
    const weekPart = h("label", { class: "field" }, "Day", weekday);
    const monthPart = h(
      "label",
      { class: "field" },
      "Day of the month (1 to 28)",
      day,
    );
    const error = h("p", { class: "error", role: "alert" });
    const showParts = () => {
      cleanPart.hidden = kind.value !== "clean";
      keyPart.hidden = kind.value !== "backup";
      weekPart.hidden = every.value !== "week";
      monthPart.hidden = every.value !== "month";
    };
    kind.addEventListener("change", showParts);
    every.addEventListener("change", showParts);
    showParts();
    const done = (answer: Job | null) => {
      modal.close();
      resolve(answer);
    };
    modal.dismissWith(() => done(null));
    modal.show(
      job ? "Edit schedule" : "New schedule",
      [
        h("label", { class: "field" }, "Name", name),
        h("label", { class: "field" }, "What", kind),
        cleanPart,
        keyPart,
        h("label", { class: "field" }, "When", every),
        weekPart,
        monthPart,
        h("label", { class: "field" }, "Time", time),
        h(
          "p",
          { class: "muted" },
          "An automatic clean removes what the scan ticks, with a backup, and never takes ownership of protected keys.",
        ),
        error,
      ],
      [
        { label: "Cancel", value: "no" },
        { label: "Save", value: "yes", kind: "primary", id: "job-save" },
      ],
      (value) => {
        if (value !== "yes") return done(null);
        const chosen: Schedule =
          every.value === "week"
            ? {
                every: "week",
                weekday: Number(weekday.value),
                time: time.value,
              }
            : every.value === "month"
              ? { every: "month", day: Number(day.value), time: time.value }
              : { every: "day", time: time.value };
        const modeValue: Mode =
          mode.value === "moderate" || mode.value === "advanced"
            ? mode.value
            : "safe";
        const input: JobInput = {
          id: job?.id ?? null,
          name: name.value,
          work:
            kind.value === "backup"
              ? { kind: "backup", key: key.value }
              : kind.value === "fullBackup"
                ? { kind: "fullBackup" }
                : {
                    kind: "clean",
                    mode: modeValue,
                    categories: [],
                    allAccounts: all.checked,
                  },
          schedule: chosen,
          enabled: job?.enabled ?? true,
        };
        api.saveJob(input).then(done, (failure: unknown) => {
          error.textContent = message(failure);
        });
      },
    );
    name.focus();
  });
}
