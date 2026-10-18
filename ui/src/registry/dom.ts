/**
 * Small DOM helpers. Text always goes in through textContent, never HTML.
 * Every id this module makes or finds carries its prefix ("reg-"), so its
 * elements never clash with another module's in the one page.
 */

export const PREFIX = "reg-";

/** Attributes that hold ids (space-separated for the aria ones). */
const ID_ATTRIBUTES = new Set([
  "id",
  "for",
  "aria-labelledby",
  "aria-describedby",
  "aria-controls",
]);

const prefixed = (name: string, value: string): string =>
  ID_ATTRIBUTES.has(name)
    ? value
        .split(" ")
        .filter((part) => part !== "")
        .map((part) => PREFIX + part)
        .join(" ")
    : value;

type Child = Node | string | null | undefined | false;

export function h<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  props: Partial<Record<string, string | boolean>> = {},
  ...children: Child[]
): HTMLElementTagNameMap[K] {
  const element = document.createElement(tag);
  for (const [name, value] of Object.entries(props)) {
    if (value === undefined || value === false) continue;
    if (name === "class") element.className = String(value);
    else if (value === true) element.setAttribute(name, "");
    else element.setAttribute(name, prefixed(name, value));
  }
  for (const child of children) {
    if (child === null || child === undefined || child === false) continue;
    element.append(
      typeof child === "string" ? document.createTextNode(child) : child,
    );
  }
  return element;
}

export function byId<T extends HTMLElement>(id: string, kind: new () => T): T {
  const element = document.getElementById(PREFIX + id);
  if (!(element instanceof kind))
    throw new Error(`#${PREFIX}${id} is missing from the page`);
  return element;
}

export function clear(element: Element): void {
  element.replaceChildren();
}
