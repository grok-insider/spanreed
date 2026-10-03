"use client";
import { jsxs, jsx } from "react/jsx-runtime";
import { Button, Tooltip, TooltipTrigger, TooltipContent } from "@fabrials/ui";
import { useCopyToClipboard } from "./code-block.js";
import { CheckIcon, CopyIcon } from "./chat-icons.js";
function join(...values) {
  return values.filter(Boolean).join(" ");
}
function ChatMessage({
  from,
  actions,
  pinActions = false,
  className,
  bodyClassName,
  children,
  ...props
}) {
  return /* @__PURE__ */ jsxs(
    "div",
    {
      className: join("fui-chat-message", className),
      "data-pin-actions": pinActions || void 0,
      "data-role": from,
      ...props,
      children: [
        /* @__PURE__ */ jsx("div", { className: join("fui-chat-message-body", bodyClassName), children }),
        actions
      ]
    }
  );
}
function MessageActions({ className, ...props }) {
  return /* @__PURE__ */ jsx("div", { className: join("fui-message-actions", className), ...props });
}
function MessageAction({ label, tooltip, children, className, ...props }) {
  const button = /* @__PURE__ */ jsx(
    Button,
    {
      "aria-label": label,
      className: join("fui-message-action", className),
      size: "icon-sm",
      type: "button",
      variant: "ghost",
      ...props,
      children
    }
  );
  return /* @__PURE__ */ jsxs(Tooltip, { children: [
    /* @__PURE__ */ jsx(TooltipTrigger, { render: button }),
    /* @__PURE__ */ jsx(TooltipContent, { children: tooltip ?? label })
  ] });
}
function CopyMessageAction({
  text,
  onCopy,
  label = "Copy",
  copiedLabel = "Copied",
  ...props
}) {
  const { copied, copy } = useCopyToClipboard();
  return /* @__PURE__ */ jsx(
    MessageAction,
    {
      ...props,
      "data-copied": copied || void 0,
      label: copied ? copiedLabel : label,
      onClick: () => void copy(typeof text === "function" ? text() : text, onCopy).catch(() => void 0),
      children: copied ? /* @__PURE__ */ jsx(CheckIcon, {}) : /* @__PURE__ */ jsx(CopyIcon, {})
    }
  );
}
function MessageTimestamp({
  dateTime,
  label,
  detail,
  className
}) {
  const time = /* @__PURE__ */ jsx("time", { className: join("fui-message-time", className), dateTime });
  if (!detail) {
    return /* @__PURE__ */ jsx("time", { className: join("fui-message-time", className), dateTime, children: label });
  }
  return /* @__PURE__ */ jsxs(Tooltip, { children: [
    /* @__PURE__ */ jsx(TooltipTrigger, { render: time, children: label }),
    /* @__PURE__ */ jsx(TooltipContent, { children: detail })
  ] });
}
export {
  ChatMessage,
  CopyMessageAction,
  MessageAction,
  MessageActions,
  MessageTimestamp
};
