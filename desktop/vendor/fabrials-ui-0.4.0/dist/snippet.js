"use client";
import { jsxs, jsx } from "react/jsx-runtime";
import { useState, useRef, useEffect } from "react";
import { Check, Copy } from "lucide-react";
import { Button } from "./controls.js";
import { classes } from "./shared.js";
function CopyButton({
  value,
  label = "Copy",
  copiedLabel = "Copied",
  onCopy,
  onCopyError,
  className,
  variant = "ghost",
  size = "icon-sm",
  ...props
}) {
  const [copied, setCopied] = useState(false);
  const timer = useRef(void 0);
  useEffect(() => () => window.clearTimeout(timer.current), []);
  async function copy() {
    try {
      await navigator.clipboard.writeText(value);
      setCopied(true);
      onCopy?.(value);
      window.clearTimeout(timer.current);
      timer.current = window.setTimeout(() => setCopied(false), 1600);
    } catch (error) {
      onCopyError?.(error);
    }
  }
  return /* @__PURE__ */ jsxs(
    Button,
    {
      type: "button",
      variant,
      size,
      "aria-label": copied ? copiedLabel : label,
      title: copied ? copiedLabel : label,
      "data-copied": copied || void 0,
      className: classes("fui-copy-button", className),
      onClick: () => void copy(),
      ...props,
      children: [
        copied ? /* @__PURE__ */ jsx(Check, { "aria-hidden": true }) : /* @__PURE__ */ jsx(Copy, { "aria-hidden": true }),
        /* @__PURE__ */ jsx("span", { className: "fui-sr-only", "aria-live": "polite", children: copied ? copiedLabel : "" })
      ]
    }
  );
}
function Snippet({
  children,
  prompt = "$",
  copyValue,
  copyLabel = "Copy command",
  label,
  className,
  ...props
}) {
  const lines = (typeof children === "string" ? children.split("\n") : [...children]).filter(
    (line, index, all) => line.length > 0 || index > 0 && index < all.length - 1
  );
  const commands = lines.filter((line) => line.trim() && !line.trimStart().startsWith("#"));
  return /* @__PURE__ */ jsxs(
    "div",
    {
      "data-slot": "snippet",
      role: "group",
      "aria-label": label,
      className: classes("fui-snippet", className),
      ...props,
      children: [
        /* @__PURE__ */ jsx("pre", { className: "fui-snippet-code", tabIndex: 0, children: /* @__PURE__ */ jsx("code", { children: lines.map((line, index) => {
          const comment = line.trimStart().startsWith("#");
          return /* @__PURE__ */ jsx(
            "span",
            {
              className: "fui-snippet-line",
              "data-comment": comment || void 0,
              "data-prompt": !comment && line.trim() && prompt ? prompt : void 0,
              children: line || " "
            },
            `${index}-${line}`
          );
        }) }) }),
        /* @__PURE__ */ jsx(
          CopyButton,
          {
            value: copyValue ?? commands.join("\n"),
            label: copyLabel,
            className: "fui-snippet-copy"
          }
        )
      ]
    }
  );
}
export {
  CopyButton,
  Snippet
};
