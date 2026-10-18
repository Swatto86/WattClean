// Building DOM without markup strings: text always goes in as text, so file
// names and paths from the machine can never become markup.

type Attributes = Record<string, string | number | boolean | null | undefined>;
type Child = Node | string | null | undefined | false;

export const el = <K extends keyof HTMLElementTagNameMap>(
  tag: K,
  attributes: Attributes = {},
  ...children: Child[]
): HTMLElementTagNameMap[K] => {
  const node = document.createElement(tag);
  for (const [name, value] of Object.entries(attributes)) {
    if (value === null || value === undefined || value === false) continue;
    if (name === "class") node.className = String(value);
    else if (name === "text") node.textContent = String(value);
    else node.setAttribute(name, value === true ? "" : String(value));
  }
  for (const child of children) {
    if (child === null || child === undefined || child === false) continue;
    node.append(child);
  }
  return node;
};

/** Sets text only when it changed, so unchanged nodes do not repaint. */
export const setText = (node: Element, text: string): void => {
  if (node.textContent !== text) node.textContent = text;
};

const SVG = "http://www.w3.org/2000/svg";

/** An icon drawn from path data the app itself defines. */
export const icon = (
  paths: readonly string[],
  className = "glyph",
): SVGSVGElement => {
  const svg = document.createElementNS(SVG, "svg");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("aria-hidden", "true");
  svg.setAttribute("class", className);
  for (const d of paths) {
    const path = document.createElementNS(SVG, "path");
    path.setAttribute("d", d);
    svg.append(path);
  }
  return svg;
};

/** Fluent-style outline icons, drawn for this app. */
export const GLYPHS = {
  menu: ["M4 6h16M4 12h16M4 18h16"],
  cleaner: [
    "M12 3c.6 4 2 5.4 6 6-4 .6-5.4 2-6 6-.6-4-2-5.4-6-6 4-.6 5.4-2 6-6z",
    "M18.5 14.5c.3 1.7.8 2.2 2.5 2.5-1.7.3-2.2.8-2.5 2.5-.3-1.7-.8-2.2-2.5-2.5 1.7-.3 2.2-.8 2.5-2.5z",
    "M5.5 15.5c.2 1.2.6 1.6 1.8 1.8-1.2.2-1.6.6-1.8 1.8-.2-1.2-.6-1.6-1.8-1.8 1.2-.2 1.6-.6 1.8-1.8z",
  ],
  shred: [
    "M7 3h7l4 4v6H6V4a1 1 0 0 1 1-1z",
    "M14 3v4h4",
    "M4 13h16",
    "M7 16v5M10 16v3M13 16v5M16 16v3",
  ],
  wipe: [
    "M4 7h16v10H4z",
    "M7 12h.01",
    "M11 10.5h6M11 13.5h4",
    "M4 20c3-1.5 6-1.5 8 0s5 1.5 8 0",
  ],
  history: ["M12 7v5l3 2", "M4.5 12a7.5 7.5 0 1 0 2.2-5.3", "M4 4v3.5h3.5"],
  settings: [
    "M12 9a3 3 0 1 0 0 6 3 3 0 0 0 0-6z",
    "M19.4 15a1.6 1.6 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.6 1.6 0 0 0-2.7 1.1V21a2 2 0 1 1-4 0v-.1a1.6 1.6 0 0 0-2.7-1.1l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1A1.6 1.6 0 0 0 3.6 15H3a2 2 0 1 1 0-4h.1a1.6 1.6 0 0 0 1.1-2.7l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1A1.6 1.6 0 0 0 9.7 4.4V3a2 2 0 1 1 4 0v.1a1.6 1.6 0 0 0 2.7 1.1l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.6 1.6 0 0 0 1.1 2.7H21a2 2 0 1 1 0 4h-.1a1.6 1.6 0 0 0-1.5 1z",
  ],
  preview: [
    "M2.5 12s3.5-6.5 9.5-6.5S21.5 12 21.5 12 18 18.5 12 18.5 2.5 12 2.5 12z",
    "M12 9.5a2.5 2.5 0 1 0 0 5 2.5 2.5 0 0 0 0-5z",
  ],
  clean: ["M4 7h16", "M9 7V4.5h6V7", "M6.5 7l1 13h9l1-13", "M10 11v5M14 11v5"],
  stop: ["M7 7h10v10H7z"],
  warning: ["M12 4l9 16H3z", "M12 10v4", "M12 17h.01"],
  plus: ["M12 5v14M5 12h14"],
  folder: [
    "M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z",
  ],
  file: ["M7 3h7l4 4v13H7z", "M14 3v4h4"],
  close: ["M6 6l12 12M18 6L6 18"],
  refresh: ["M20 11a8 8 0 1 0-2.3 5.7", "M20 5v6h-6"],
  download: ["M12 4v11", "M7 10l5 5 5-5", "M5 20h14"],
  shield: ["M12 3l7 3v6c0 4.5-3 7.5-7 9-4-1.5-7-4.5-7-9V6z", "M9 12l2 2 4-4"],
  drive: ["M3 13l2.5-7h13L21 13v5H3z", "M3 13h18", "M17 16h.01"],
  registry: [
    "M4 4h7v7H4z",
    "M13 4h7v7h-7z",
    "M4 13h7v7H4z",
    "M14 16.5l2 2 4-4",
  ],
  search: [
    "M10.5 4a6.5 6.5 0 1 0 0 13 6.5 6.5 0 0 0 0-13z",
    "M15.5 15.5L20 20",
  ],
  tuner: ["M6 4v16M12 4v16M18 4v16", "M4 9h4M10 15h4M16 7h4"],
  watcher: ["M3 12h4l2-5 4 10 2-5h6"],
  backup: ["M4 5h16v4H4z", "M5 9v10h14V9", "M10 13h4"],
  schedule: ["M4 6h16v14H4z", "M4 10h16", "M8 3v4M16 3v4", "M8 14h3M8 17h6"],
  programs: ["M4 4h6v6H4z", "M14 4h6v6h-6z", "M4 14h6v6H4z", "M14 17h6"],
  trace: ["M4 18c4 0 4-12 8-12s4 12 8 12", "M4 18h.01M12 6h.01M20 18h.01"],
  forced: ["M13 3l8 8-3 3-8-8z", "M10 9l-7 7 3 3 7-7", "M15 19h6"],
} as const;

export type GlyphName = keyof typeof GLYPHS;

export const glyph = (name: GlyphName, className = "glyph"): SVGSVGElement =>
  icon(GLYPHS[name], className);

/** A toolbar button with an icon and a label. */
export const commandButton = (
  id: string,
  label: string,
  name: GlyphName,
  kind: "command" | "primary" | "primary danger" | "secondary" = "command",
): HTMLButtonElement =>
  el(
    "button",
    { id, type: "button", class: kind },
    glyph(name),
    el("span", { text: label }),
  );

let noticeTimer: number | undefined;

/** A short message at the bottom of the window. */
export const notice = (text: string, error = false): void => {
  const box = document.querySelector<HTMLElement>("#notice");
  if (!box) return;
  box.textContent = text;
  box.classList.toggle("error", error);
  box.hidden = false;
  window.clearTimeout(noticeTimer);
  noticeTimer = window.setTimeout(
    () => {
      box.hidden = true;
    },
    error ? 8000 : 4000,
  );
};
