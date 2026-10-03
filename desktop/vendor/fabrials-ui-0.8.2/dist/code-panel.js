"use client";
import { jsxs, jsx } from "react/jsx-runtime";
import { useState, useSyncExternalStore, useEffect } from "react";
import { Tabs } from "@base-ui/react/tabs";
import { Palette, Braces, Terminal, FileCode2, FileText } from "lucide-react";
import { CopyButton } from "./snippet.js";
import { classes } from "./shared.js";
import { PACKAGE_MANAGERS, packageCommand, highlightCode } from "./syntax.js";
const ICONS = {
  bash: Terminal,
  sh: Terminal,
  shell: Terminal,
  zsh: Terminal,
  json: Braces,
  jsonc: Braces,
  css: Palette
};
function useCopyStatus() {
  const [status, setStatus] = useState("");
  return {
    status,
    onCopy: () => setStatus("Copied to clipboard"),
    onCopyError: () => setStatus("Couldn't copy. Select the code and copy it.")
  };
}
function CopyStatus({ status }) {
  return /* @__PURE__ */ jsx("span", { role: "status", className: status.startsWith("Couldn't") ? "fui-code-panel-status" : "fui-sr-only", children: status });
}
function languageIcon(language) {
  const Icon = ICONS[(language ?? "").toLowerCase()] ?? (language ? FileCode2 : FileText);
  return /* @__PURE__ */ jsx(Icon, { "aria-hidden": true, size: 14 });
}
function Words({ text, words, type }) {
  if (!words?.length) return /* @__PURE__ */ jsx("span", { className: type ? `fui-token-${type}` : void 0, children: text });
  const pattern = new RegExp(`(${words.map((w) => w.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")).join("|")})`, "g");
  return /* @__PURE__ */ jsx("span", { className: type ? `fui-token-${type}` : void 0, children: text.split(pattern).map(
    (part, index) => words.includes(part) ? /* @__PURE__ */ jsx("mark", { className: "fui-code-word", children: part }, index) : part
  ) });
}
function CodeLines({
  code,
  language,
  highlightLines,
  addedLines,
  removedLines,
  highlightWords
}) {
  const lines = highlightCode(code.replace(/\n$/, ""), language);
  const diff = Boolean(addedLines?.length || removedLines?.length);
  return /* @__PURE__ */ jsx("code", { "data-diff-gutter": diff || void 0, children: lines.map((tokens, index) => {
    const n = index + 1;
    const mark = addedLines?.includes(n) ? "added" : removedLines?.includes(n) ? "removed" : void 0;
    const Line = mark === "added" ? "ins" : mark === "removed" ? "del" : "span";
    return /* @__PURE__ */ jsx(
      Line,
      {
        className: "fui-code-line",
        "data-highlighted": highlightLines?.includes(n) || void 0,
        "data-diff": mark,
        children: tokens.length ? tokens.map((token, t) => /* @__PURE__ */ jsx(Words, { text: token.text, words: highlightWords, type: token.type }, t)) : (
          // A plain space keeps an empty line's height; a zero-width space would paste into code as an invalid character.
          " "
        )
      },
      index
    );
  }) });
}
function CodePanel({
  code,
  language,
  title,
  icon,
  lineNumbers = false,
  highlightLines,
  addedLines,
  removedLines,
  highlightWords,
  copyValue,
  children,
  bare = false,
  className,
  ...props
}) {
  const copy = copyValue ?? (removedLines?.length ? code.split("\n").filter((_, i) => !removedLines.includes(i + 1)).join("\n") : code);
  const copied = useCopyStatus();
  return /* @__PURE__ */ jsxs(
    "figure",
    {
      "data-slot": "code-panel",
      "data-bare": bare || void 0,
      "data-line-numbers": lineNumbers || void 0,
      className: classes("fui-code-panel", className),
      ...props,
      children: [
        bare ? null : /* @__PURE__ */ jsxs("figcaption", { className: "fui-code-panel-bar", children: [
          /* @__PURE__ */ jsxs("span", { className: "fui-code-panel-title", children: [
            icon ?? languageIcon(language),
            /* @__PURE__ */ jsx("span", { children: title ?? language ?? "Code" })
          ] }),
          /* @__PURE__ */ jsx(CopyButton, { value: copy, label: "Copy code", onCopy: copied.onCopy, onCopyError: copied.onCopyError })
        ] }),
        /* @__PURE__ */ jsx("pre", { className: "fui-code-panel-body", tabIndex: 0, children: children ?? /* @__PURE__ */ jsx(
          CodeLines,
          {
            code,
            language,
            highlightLines,
            addedLines,
            removedLines,
            highlightWords
          }
        ) }),
        bare ? /* @__PURE__ */ jsx(CopyButton, { value: copy, label: "Copy code", className: "fui-code-panel-float", onCopy: copied.onCopy, onCopyError: copied.onCopyError }) : null,
        /* @__PURE__ */ jsx(CopyStatus, { status: copied.status })
      ]
    }
  );
}
function CodeTabs({
  items,
  defaultValue,
  value,
  onValueChange,
  label = "Code examples",
  className
}) {
  const [internal, setInternal] = useState(defaultValue ?? items[0]?.value);
  const copied = useCopyStatus();
  const current = value ?? internal;
  const active = items.find((item) => item.value === current) ?? items[0];
  return /* @__PURE__ */ jsxs(
    Tabs.Root,
    {
      value: current,
      onValueChange: (next) => {
        setInternal(next);
        onValueChange?.(next);
      },
      "data-slot": "code-tabs",
      className: classes("fui-code-panel", "fui-code-tabs", className),
      children: [
        /* @__PURE__ */ jsxs("div", { className: "fui-code-panel-bar", children: [
          /* @__PURE__ */ jsx(Tabs.List, { "aria-label": label, className: "fui-code-tabs-list", children: items.map((item) => /* @__PURE__ */ jsx(Tabs.Tab, { value: item.value, className: "fui-code-tab", children: item.label }, item.value)) }),
          active && /* @__PURE__ */ jsx(CopyButton, { value: active.copyValue ?? active.code, label: "Copy code", onCopy: copied.onCopy, onCopyError: copied.onCopyError })
        ] }),
        items.map(({ value: tab, label: _label, code, language, lineNumbers, highlightLines, addedLines, removedLines, highlightWords, children }) => /* @__PURE__ */ jsx(Tabs.Panel, { value: tab, className: "fui-code-tabs-panel", "data-line-numbers": lineNumbers || void 0, children: /* @__PURE__ */ jsx("pre", { className: "fui-code-panel-body", tabIndex: 0, children: children ?? /* @__PURE__ */ jsx(
          CodeLines,
          {
            code,
            language,
            highlightLines,
            addedLines,
            removedLines,
            highlightWords
          }
        ) }) }, tab)),
        /* @__PURE__ */ jsx(CopyStatus, { status: copied.status })
      ]
    }
  );
}
let chosen = "npm";
const listeners = /* @__PURE__ */ new Set();
const managerStore = {
  subscribe(listener) {
    listeners.add(listener);
    return () => listeners.delete(listener);
  },
  get: () => chosen,
  set(next) {
    chosen = next;
    listeners.forEach((listener) => listener());
  }
};
function PackageInstall({
  command,
  kind = "run",
  persistKey = "fui-package-manager",
  className
}) {
  const manager = useSyncExternalStore(managerStore.subscribe, managerStore.get, () => "npm");
  useEffect(() => {
    if (!persistKey) return;
    try {
      const saved = window.localStorage.getItem(persistKey);
      if (saved && saved in PACKAGE_MANAGERS && saved !== managerStore.get()) managerStore.set(saved);
    } catch {
    }
  }, [persistKey]);
  return /* @__PURE__ */ jsx(
    CodeTabs,
    {
      className: classes("fui-package-install", className),
      label: "Package manager",
      value: manager,
      onValueChange: (next) => {
        managerStore.set(next);
        if (persistKey)
          try {
            window.localStorage.setItem(persistKey, next);
          } catch {
          }
      },
      items: Object.keys(PACKAGE_MANAGERS).map((name) => ({
        value: name,
        label: name,
        code: packageCommand(name, command, kind),
        language: "bash"
      }))
    }
  );
}
export {
  CodePanel,
  CodeTabs,
  PackageInstall
};
