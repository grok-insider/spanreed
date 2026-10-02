"use client";
import { jsxs, jsx } from "react/jsx-runtime";
import { useId, useRef, useState } from "react";
import { X } from "lucide-react";
import { buttonVariants } from "./button-variants.js";
import { IconButton } from "./icon-button.js";
import { classes } from "./shared.js";
import { TruncatedText } from "./truncated-text.js";
function FileInput({
  label,
  onFilesChange,
  fileName,
  onClear,
  clearLabel = "Remove file",
  description,
  error,
  variant = "outline",
  size = "default",
  className,
  buttonClassName,
  onChange,
  id,
  disabled,
  ref,
  "aria-describedby": describedBy,
  ...input
}) {
  const generated = useId();
  const base = id ?? generated;
  const own = useRef(null);
  const [picked, setPicked] = useState(null);
  const shown = fileName !== void 0 ? fileName : picked;
  const hasName = shown !== null && shown !== void 0 && shown !== false && shown !== "";
  const ids = [
    describedBy,
    hasName ? `${base}-name` : void 0,
    description ? `${base}-description` : void 0,
    error ? `${base}-error` : void 0
  ].filter(Boolean).join(" ");
  return /* @__PURE__ */ jsxs("div", { className: classes("fui-file-input", className), "data-invalid": error ? "" : void 0, "data-disabled": disabled ? "" : void 0, children: [
    /* @__PURE__ */ jsxs("div", { className: "fui-file-input-row", children: [
      /* @__PURE__ */ jsxs(
        "label",
        {
          className: classes(buttonVariants({ variant, size }), "fui-file-input-button", buttonClassName),
          "data-variant": variant,
          "data-size": size,
          children: [
            /* @__PURE__ */ jsx(
              "input",
              {
                ...input,
                id: base,
                type: "file",
                disabled,
                className: "fui-sr-only",
                "aria-describedby": ids || void 0,
                "aria-invalid": error ? true : input["aria-invalid"],
                ref: (node) => {
                  own.current = node;
                  if (typeof ref === "function") return ref(node);
                  if (ref) ref.current = node;
                },
                onChange: (event) => {
                  onChange?.(event);
                  const files = Array.from(event.currentTarget.files ?? []);
                  setPicked(files.length ? files.map((file) => file.name).join(", ") : null);
                  onFilesChange?.(files);
                }
              }
            ),
            label
          ]
        }
      ),
      hasName ? /* @__PURE__ */ jsx("span", { className: "fui-file-input-name", id: `${base}-name`, "aria-live": "polite", children: typeof shown === "string" ? /* @__PURE__ */ jsx(TruncatedText, { children: shown }) : shown }) : null,
      onClear && hasName ? /* @__PURE__ */ jsx(
        IconButton,
        {
          type: "button",
          size: "icon-sm",
          label: clearLabel,
          disabled,
          onClick: () => {
            if (own.current) own.current.value = "";
            setPicked(null);
            onClear();
            own.current?.focus();
          },
          children: /* @__PURE__ */ jsx(X, { "aria-hidden": true })
        }
      ) : null
    ] }),
    description ? /* @__PURE__ */ jsx("p", { className: "fui-description", id: `${base}-description`, children: description }) : null,
    error ? /* @__PURE__ */ jsx("p", { className: "fui-field-error", id: `${base}-error`, role: "alert", children: error }) : null
  ] });
}
export {
  FileInput
};
