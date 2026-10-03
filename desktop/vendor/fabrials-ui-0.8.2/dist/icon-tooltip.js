"use client";
import { jsxs, jsx } from "react/jsx-runtime";
import { KbdGroup, Kbd } from "./kbd.js";
import { TooltipContent } from "./menu.js";
const KEY_NAMES = {
  "⌘": "Meta",
  "⌥": "Alt",
  "⇧": "Shift",
  "⌃": "Control",
  Ctrl: "Control",
  Cmd: "Meta",
  Esc: "Escape",
  "↵": "Enter",
  Del: "Delete"
};
function shortcutParts(shortcut) {
  if (!shortcut) return null;
  if (typeof shortcut === "string") return { keys: [shortcut], sequence: false, separator: void 0 };
  if (Array.isArray(shortcut)) return { keys: shortcut, sequence: false, separator: void 0 };
  const object = shortcut;
  return { keys: object.keys, sequence: Boolean(object.sequence), separator: object.separator };
}
function keyShortcutsValue(shortcut) {
  const parts = shortcutParts(shortcut);
  if (!parts || parts.sequence || parts.keys.length === 0) return void 0;
  return parts.keys.map((key) => KEY_NAMES[key] ?? key).join("+");
}
function IconTooltipContent({ label, shortcut }) {
  const parts = shortcutParts(shortcut);
  const keys = parts?.keys;
  return /* @__PURE__ */ jsxs(TooltipContent, { children: [
    label,
    keys?.length ? /* @__PURE__ */ jsx(KbdGroup, { className: "fui-icon-button-keys", sequence: parts?.sequence, separator: parts?.separator ?? "then", children: keys.map((key, index) => /* @__PURE__ */ jsx(Kbd, { children: key }, `${key}-${index}`)) }) : null
  ] });
}
export {
  IconTooltipContent,
  keyShortcutsValue
};
