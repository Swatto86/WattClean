/** Accessible in-app dialogs: focus kept inside, Esc to dismiss where allowed. */
import { byId, h } from "./dom.ts";

export interface Action {
  label: string;
  value: string;
  kind?: "primary" | "danger" | "ghost";
  id?: string;
}

/** One open modal whose content can change step by step (the wizard). */
export class Modal {
  readonly root: HTMLDivElement;
  private readonly box: HTMLDivElement;
  private readonly before: Element | null;
  private onEscape: (() => void) | null = null;

  constructor(label: string, wide = false) {
    this.before = document.activeElement;
    this.box = h("div", {
      class: wide ? "dialog wide" : "dialog",
      role: "dialog",
      "aria-modal": "true",
      "aria-label": label,
    });
    // Focusable, so a click on the dialog's text or around it keeps focus,
    // and with it Esc and Tab, in the dialog rather than the page behind.
    this.root = h("div", { class: "backdrop", tabindex: "-1" }, this.box);
    this.root.addEventListener("keydown", (event) => this.keydown(event));
    byId("dialog-root", HTMLDivElement).append(this.root);
  }

  /** Replace the content. Buttons resolve `choose` with their value. */
  show(
    title: string,
    body: Node[],
    actions: Action[],
    choose: (value: string) => void,
  ): void {
    const footer = h("footer");
    for (const action of actions) {
      const button = h(
        "button",
        { class: action.kind ?? "ghost", type: "button", id: action.id },
        action.label,
      );
      button.addEventListener("click", () => choose(action.value));
      footer.append(button);
    }
    this.box.replaceChildren(
      h("header", {}, h("h2", {}, title)),
      h("div", { class: "body" }, ...body),
    );
    if (actions.length > 0) this.box.append(footer);
    const preferred = this.box.querySelector<HTMLElement>(
      "button.primary, button.danger, input, button",
    );
    preferred?.focus();
  }

  dismissWith(handler: (() => void) | null): void {
    this.onEscape = handler;
  }

  close(): void {
    this.root.remove();
    if (this.before instanceof HTMLElement) this.before.focus();
  }

  private keydown(event: KeyboardEvent): void {
    if (event.key === "Escape" && this.onEscape) {
      event.preventDefault();
      this.onEscape();
      return;
    }
    if (event.key !== "Tab") return;
    const focusable = [
      ...this.box.querySelectorAll<HTMLElement>(
        "button:not(:disabled), input, select, [tabindex='0']",
      ),
    ];
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    if (!first || !last) return;
    const active = document.activeElement;
    const inside = active !== null && this.box.contains(active);
    if (event.shiftKey && (!inside || active === first)) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && (!inside || active === last)) {
      event.preventDefault();
      first.focus();
    }
  }
}

/** Ask a yes/no question; resolves true only for the confirming button. */
export function confirm(
  title: string,
  message: string,
  yes: string,
  kind: "primary" | "danger" = "primary",
): Promise<boolean> {
  return new Promise((resolve) => {
    const modal = new Modal(title);
    const done = (answer: boolean) => {
      modal.close();
      resolve(answer);
    };
    modal.dismissWith(() => done(false));
    modal.show(
      title,
      [h("p", {}, message)],
      [
        { label: "Cancel", value: "no" },
        { label: yes, value: "yes", kind },
      ],
      (value) => done(value === "yes"),
    );
  });
}

export function inform(title: string, message: string): Promise<void> {
  return new Promise((resolve) => {
    const modal = new Modal(title);
    const done = () => {
      modal.close();
      resolve();
    };
    modal.dismissWith(done);
    modal.show(
      title,
      [h("p", {}, message)],
      [{ label: "OK", value: "ok", kind: "primary" }],
      done,
    );
  });
}
