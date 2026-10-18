/**
 * Right-click menus. The browser's menu never opens: every right-click (and
 * the menu key) is answered with a Windows popup menu that Rust shows. On
 * text it is Windows' own edit menu; on a list it is that list's menu.
 */
import { api, message } from "./api.ts";
import { inform } from "./dialog.ts";
import { editMenuFor, isTextField, type EditMenu } from "./editmenu.ts";

export type MenuEntry =
  | { kind: "item"; id: string; text: string; enabled?: boolean }
  | { kind: "separator" }
  | { kind: "cut" | "copy" | "paste" | "selectAll"; enabled: boolean };

export interface Point {
  x: number;
  y: number;
}

/** An item; `&` marks the letter that chooses it from the keyboard. */
export function item(id: string, text: string, enabled = true): MenuEntry {
  return { kind: "item", id, text, enabled };
}

export const separator: MenuEntry = { kind: "separator" };

/** Show a native menu; resolves with the chosen item's id, or null. */
export async function popup(
  entries: MenuEntry[],
  at: Point,
): Promise<string | null> {
  try {
    return await api.contextMenu(entries, at.x, at.y);
  } catch (error: unknown) {
    await inform("The menu could not be shown", message(error));
    return null;
  }
}

/**
 * Where the menu opens: at the pointer, or under the focused element when
 * the menu key or Shift+F10 opened it (the event then has no position).
 */
export function pointFor(event: MouseEvent, element: Element): Point {
  if (event.clientX > 0 || event.clientY > 0) {
    return { x: event.clientX, y: event.clientY };
  }
  const box = element.getBoundingClientRect();
  return { x: box.left + 12, y: box.bottom };
}

/** Given the element right-clicked inside the marked list. */
type Provider = (target: Element, at: Point) => void;
const providers = new Map<string, Provider>();

/** Answer right-clicks inside elements marked `data-menu="<name>"`. */
export function onMenu(name: string, provider: Provider): void {
  providers.set(name, provider);
}

function editEntries(menu: EditMenu): MenuEntry[] {
  const entries: MenuEntry[] = [];
  for (const kind of ["cut", "copy", "paste"] as const) {
    const enabled = menu[kind];
    if (enabled !== undefined) entries.push({ kind, enabled });
  }
  if (menu.selectAll !== undefined) {
    if (entries.length > 0) entries.push(separator);
    entries.push({ kind: "selectAll", enabled: menu.selectAll });
  }
  return entries;
}

/** One listener for the whole page; installed once at start-up. */
export function installMenus(): void {
  document.addEventListener("contextmenu", (event) => {
    event.preventDefault();
    const target = event.target;
    if (!(target instanceof Element)) return;
    const edit = editMenuFor(target, document.getSelection()?.toString() ?? "");
    const owner = target.closest<HTMLElement>("[data-menu]");
    const provider = owner ? providers.get(owner.dataset["menu"] ?? "") : null;
    // A text field, or text selected outside a list, gets the edit menu.
    if (edit && (!provider || isTextField(target))) {
      void popup(editEntries(edit), pointFor(event, target));
      return;
    }
    if (provider) provider(target, pointFor(event, target));
  });
}
