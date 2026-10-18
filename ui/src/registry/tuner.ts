/** The Registry Tuner view: tweaks grouped by area, each a switch. */
import { api, message, type Tweak } from "./api.ts";
import { confirm, inform } from "./dialog.ts";
import { byId, h } from "./dom.ts";

const GROUP_TITLES: Record<Tweak["group"], string> = {
  appearance: "Windows appearance",
  explorer: "File Explorer",
  taskbar: "Taskbar and Start",
  performance: "Performance",
  privacy: "Privacy",
  security: "Security and stability",
  updates: "Updates",
};

const NEEDS_TEXT: Record<Tweak["needs"], string> = {
  nothing: "",
  explorer: "restarting File Explorer",
  signOut: "signing out",
  restart: "restarting Windows",
};

export interface TunerView {
  load: () => Promise<void>;
}

export function wireTuner(): TunerView {
  const listing = byId("tuner-list", HTMLDivElement);
  const pending = byId("tuner-pending", HTMLParagraphElement);
  const filter = byId("tuner-filter", HTMLInputElement);
  const waiting = new Set<Tweak["needs"]>();
  let tweaks: Tweak[] = [];

  const showPending = () => {
    const needs = [...waiting].map((need) => NEEDS_TEXT[need]).filter(Boolean);
    pending.hidden = needs.length === 0;
    pending.textContent = `Some changes take effect after ${needs.join(", or ")}.`;
  };

  const row = (tweak: Tweak): HTMLElement => {
    const toggle = h("input", {
      type: "checkbox",
      role: "switch",
      id: `tweak-${tweak.id}`,
      disabled: tweak.unavailable !== null,
      "aria-describedby": `tweak-${tweak.id}-about`,
    });
    toggle.checked = tweak.on;
    toggle.addEventListener("change", () => void flip(tweak, toggle));
    return h(
      "div",
      { class: "tweak", "data-id": tweak.id },
      h(
        "div",
        { class: "tweak-text" },
        h("label", { for: `tweak-${tweak.id}` }, h("b", {}, tweak.title)),
        h(
          "p",
          { class: "muted", id: `tweak-${tweak.id}-about` },
          tweak.description,
        ),
        h(
          "div",
          { class: "tags" },
          tweak.needs !== "nothing"
            ? h("span", { class: "chip" }, `Needs ${NEEDS_TEXT[tweak.needs]}`)
            : null,
          tweak.machine
            ? h("span", { class: "chip admin" }, "All accounts · Admin")
            : null,
          tweak.changedByApp
            ? h("span", { class: "chip changed" }, "Changed by WattClean")
            : null,
          tweak.unavailable
            ? h("span", { class: "chip" }, tweak.unavailable)
            : null,
        ),
      ),
      h("span", { class: "switch", "aria-hidden": "true" }),
      toggle,
    );
  };

  const render = () => {
    const words = filter.value.trim().toLowerCase();
    const shown = tweaks.filter(
      (t) =>
        words === "" ||
        `${t.title} ${t.description}`.toLowerCase().includes(words),
    );
    const groups: HTMLElement[] = [];
    for (const [group, title] of Object.entries(GROUP_TITLES)) {
      const members = shown.filter((t) => t.group === group);
      if (members.length === 0) continue;
      groups.push(
        h(
          "section",
          { class: "card tweak-group" },
          h("h2", {}, title),
          ...members.map(row),
        ),
      );
    }
    listing.replaceChildren(
      ...(groups.length > 0
        ? groups
        : [h("p", { class: "empty" }, "No tweak matches that filter.")]),
    );
  };

  const flip = async (tweak: Tweak, toggle: HTMLInputElement) => {
    toggle.disabled = true;
    try {
      const next = await api.setTweak(tweak.id, toggle.checked);
      tweaks = tweaks.map((t) => (t.id === next.id ? next : t));
      if (next.needs !== "nothing") waiting.add(next.needs);
      showPending();
    } catch (error: unknown) {
      await inform("The setting was not changed", message(error));
    }
    render();
    byId(`tweak-${tweak.id}`, HTMLInputElement).focus();
  };

  const load = async () => {
    try {
      tweaks = await api.listTweaks();
      render();
    } catch (error: unknown) {
      listing.replaceChildren(h("p", { class: "error" }, message(error)));
    }
  };

  filter.addEventListener("input", render);
  byId("tuner-explorer", HTMLButtonElement).addEventListener("click", () => {
    void (async () => {
      const yes = await confirm(
        "Restart File Explorer",
        "The taskbar disappears for a moment and open File Explorer windows close.",
        "Restart",
      );
      if (!yes) return;
      try {
        await api.restartExplorer();
        waiting.delete("explorer");
        showPending();
      } catch (error: unknown) {
        await inform("File Explorer was not restarted", message(error));
      }
    })();
  });
  /** Sign out or restart Windows, once the person agrees. */
  const session = (
    id: string,
    title: string,
    text: string,
    act: () => Promise<void>,
  ) => {
    byId(id, HTMLButtonElement).addEventListener("click", () => {
      void (async () => {
        if (!(await confirm(title, text, title, "danger"))) return;
        try {
          await act();
        } catch (error: unknown) {
          await inform(`${title} did not happen`, message(error));
        }
      })();
    });
  };
  session(
    "tuner-sign-out",
    "Sign out",
    "Sign out of Windows now? Programs still open are closed: save your work first.",
    api.signOut,
  );
  session(
    "tuner-restart",
    "Restart PC",
    "Restart Windows now? Programs still open are closed: save your work first.",
    api.restartPc,
  );
  byId("tuner-reset", HTMLButtonElement).addEventListener("click", () => {
    void (async () => {
      const yes = await confirm(
        "Undo all changes",
        "Put back every setting WattClean changed, exactly as it was before?",
        "Undo all",
      );
      if (!yes) return;
      try {
        const outcome = await api.resetTweaks();
        const lines = [`${outcome.reset} settings put back.`];
        for (const [what, why] of outcome.failed) lines.push(`${what}: ${why}`);
        await inform("Changes undone", lines.join("\n"));
      } catch (error: unknown) {
        await inform("Nothing was undone", message(error));
      }
      await load();
    })();
  });
  return { load };
}
