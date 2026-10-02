"use client";
import { jsxs, jsx } from "react/jsx-runtime";
import { useState, useRef, useCallback, useEffect, useLayoutEffect } from "react";
import { Tooltip, TooltipTrigger, Button, TooltipContent, Spinner } from "@fabrials/ui";
import { SquareIcon, ArrowUpIcon } from "./chat-icons.js";
const useIsomorphicLayoutEffect = typeof window === "undefined" ? useEffect : useLayoutEffect;
function assignRef(ref, value) {
  if (typeof ref === "function") ref(value);
  else if (ref) ref.current = value;
}
function ChatComposer({
  value: valueProp,
  defaultValue = "",
  onValueChange,
  onSubmit,
  onStop,
  status = "ready",
  queueWhileBusy = false,
  disabled = false,
  submitDisabled = false,
  allowEmptySubmit = false,
  placeholder = "Message",
  label = "Message",
  sendLabel = "Send message",
  queueLabel = "Queue message",
  stopLabel = "Stop",
  attachments,
  tools,
  trailing,
  footer,
  onPasteFiles,
  onRemoveLastAttachment,
  textareaRef,
  textareaProps,
  maxHeight = "min(40vh, 16rem)",
  className
}) {
  const [inner, setInner] = useState(defaultValue);
  const value = valueProp ?? inner;
  const local = useRef(null);
  const composing = useRef(false);
  const busy = status === "submitting" || status === "streaming";
  const empty = value.trim().length === 0;
  const blocked = disabled || submitDisabled || empty && !allowEmptySubmit || busy && !queueWhileBusy;
  const setValue = useCallback(
    (next) => {
      if (valueProp === void 0) setInner(next);
      onValueChange?.(next);
    },
    [valueProp, onValueChange]
  );
  useIsomorphicLayoutEffect(() => {
    const node = local.current;
    if (!node || typeof CSS === "undefined" || CSS.supports?.("field-sizing", "content")) return;
    node.style.height = "auto";
    node.style.height = `${node.scrollHeight}px`;
  }, [value]);
  const submit = (event) => {
    event?.preventDefault();
    if (blocked) return;
    onSubmit(value);
  };
  const onKeyDown = (event) => {
    textareaProps?.onKeyDown?.(event);
    if (event.defaultPrevented) return;
    if (event.key === "Enter" && !event.shiftKey) {
      if (composing.current || event.nativeEvent.isComposing) return;
      event.preventDefault();
      submit();
      return;
    }
    if (event.key === "Backspace" && event.currentTarget.value === "" && onRemoveLastAttachment) {
      event.preventDefault();
      onRemoveLastAttachment();
    }
  };
  const onPaste = (event) => {
    textareaProps?.onPaste?.(event);
    if (event.defaultPrevented || !onPasteFiles) return;
    const files = [];
    for (const item of Array.from(event.clipboardData?.items ?? [])) {
      if (item.kind !== "file") continue;
      const file = item.getAsFile();
      if (file) files.push(file);
    }
    if (files.length > 0) {
      event.preventDefault();
      onPasteFiles(files);
    }
  };
  const sendButton = /* @__PURE__ */ jsx(
    Button,
    {
      "aria-label": busy ? queueLabel : sendLabel,
      className: "fui-composer-send",
      "data-busy": busy || void 0,
      disabled: blocked,
      size: "icon-sm",
      type: "submit",
      variant: "default",
      children: status === "submitting" && !onStop ? /* @__PURE__ */ jsx(Spinner, { "aria-hidden": true, label: "", role: "presentation" }) : /* @__PURE__ */ jsx(ArrowUpIcon, {})
    }
  );
  return /* @__PURE__ */ jsxs(
    "form",
    {
      "aria-busy": busy || void 0,
      className: ["fui-composer", className].filter(Boolean).join(" "),
      "data-disabled": disabled || void 0,
      "data-status": status,
      onSubmit: submit,
      children: [
        attachments ? /* @__PURE__ */ jsx("div", { className: "fui-composer-attachments", children: attachments }) : null,
        /* @__PURE__ */ jsx(
          "textarea",
          {
            ...textareaProps,
            "aria-label": label,
            className: "fui-composer-input",
            disabled,
            name: textareaProps?.name ?? "message",
            onChange: (event) => setValue(event.currentTarget.value),
            onCompositionEnd: (event) => {
              composing.current = false;
              textareaProps?.onCompositionEnd?.(event);
            },
            onCompositionStart: (event) => {
              composing.current = true;
              textareaProps?.onCompositionStart?.(event);
            },
            onKeyDown,
            onPaste,
            placeholder,
            ref: (node) => {
              local.current = node;
              assignRef(textareaRef, node);
            },
            rows: textareaProps?.rows ?? 1,
            style: { maxHeight, ...textareaProps?.style },
            value
          }
        ),
        /* @__PURE__ */ jsxs("div", { className: "fui-composer-toolbar", children: [
          /* @__PURE__ */ jsx("div", { className: "fui-composer-tools", children: tools }),
          /* @__PURE__ */ jsxs("div", { className: "fui-composer-actions", children: [
            trailing,
            busy && onStop ? /* @__PURE__ */ jsxs(Tooltip, { children: [
              /* @__PURE__ */ jsx(
                TooltipTrigger,
                {
                  render: /* @__PURE__ */ jsx(Button, { "aria-label": stopLabel, className: "fui-composer-stop", onClick: onStop, size: "icon-sm", type: "button", variant: "outline", children: /* @__PURE__ */ jsx(SquareIcon, {}) })
                }
              ),
              /* @__PURE__ */ jsx(TooltipContent, { children: stopLabel })
            ] }) : null,
            busy && onStop && !queueWhileBusy ? null : sendButton
          ] })
        ] }),
        footer ? /* @__PURE__ */ jsx("div", { className: "fui-composer-footer", children: footer }) : null
      ]
    }
  );
}
function ComposerToggle({
  pressed,
  onPressedChange,
  icon,
  children,
  label,
  tooltip,
  disabled
}) {
  const button = /* @__PURE__ */ jsxs(
    "button",
    {
      "aria-label": label,
      "aria-pressed": pressed,
      className: "fui-composer-pill",
      "data-icon-only": children ? void 0 : true,
      "data-state": pressed ? "on" : "off",
      disabled,
      onClick: () => onPressedChange(!pressed),
      type: "button",
      children: [
        icon,
        children ? /* @__PURE__ */ jsx("span", { children }) : null
      ]
    }
  );
  if (!tooltip) return button;
  return /* @__PURE__ */ jsxs(Tooltip, { children: [
    /* @__PURE__ */ jsx(TooltipTrigger, { render: button }),
    /* @__PURE__ */ jsx(TooltipContent, { children: tooltip })
  ] });
}
function ComposerButton({
  icon,
  children,
  label,
  tooltip,
  disabled,
  onClick
}) {
  const button = /* @__PURE__ */ jsxs(
    "button",
    {
      "aria-label": label,
      className: "fui-composer-pill",
      "data-icon-only": children ? void 0 : true,
      disabled,
      onClick,
      type: "button",
      children: [
        icon,
        children ? /* @__PURE__ */ jsx("span", { children }) : null
      ]
    }
  );
  if (!tooltip) return button;
  return /* @__PURE__ */ jsxs(Tooltip, { children: [
    /* @__PURE__ */ jsx(TooltipTrigger, { render: button }),
    /* @__PURE__ */ jsx(TooltipContent, { children: tooltip })
  ] });
}
export {
  ChatComposer,
  ComposerButton,
  ComposerToggle
};
