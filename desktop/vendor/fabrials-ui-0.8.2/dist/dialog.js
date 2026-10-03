"use client";
import { jsx, jsxs } from "react/jsx-runtime";
import { useContext, useEffect, useState, useCallback, useRef, useLayoutEffect, createContext } from "react";
import { createPortal } from "react-dom";
import { Dialog as Dialog$1 } from "@base-ui/react/dialog";
import { AlertDialog as AlertDialog$1 } from "@base-ui/react/alert-dialog";
import { X } from "lucide-react";
import { Button } from "./controls.js";
import { classes } from "./shared.js";
function assignRef(ref, value) {
  if (typeof ref === "function") ref(value);
  else if (ref) ref.current = value;
}
function guardComposition(onOpenChange) {
  return (open, details) => {
    if (!open && details.reason === "escape-key" && "isComposing" in details.event && details.event.isComposing) {
      details.cancel();
      return;
    }
    onOpenChange?.(open, details);
  };
}
function Dialog({
  onOpenChange,
  ...props
}) {
  return /* @__PURE__ */ jsx(Dialog$1.Root, { ...props, onOpenChange: guardComposition(onOpenChange) });
}
const DialogTrigger = Dialog$1.Trigger;
const DialogClose = Dialog$1.Close;
const DialogPortal = Dialog$1.Portal;
function DialogOverlay({ className, ...props }) {
  return /* @__PURE__ */ jsx(Dialog$1.Backdrop, { className: classes("fui-backdrop", className), ...props });
}
const ChromeContext = createContext(null);
function DialogContent({
  className,
  children,
  closeLabel = "Close",
  closeVariant = "secondary",
  container,
  showCloseButton = true,
  close,
  placement = "center",
  size = "default",
  padding = "default",
  keepMounted,
  backdrop = true,
  ...props
}) {
  const mode = close ?? (showCloseButton ? "corner" : "none");
  const [slotUsers, setSlotUsers] = useState(0);
  const addSlotUser = useCallback(() => {
    setSlotUsers((count) => count + 1);
    return () => setSlotUsers((count) => count - 1);
  }, []);
  const [slot, setSlot] = useState(null);
  return /* @__PURE__ */ jsxs(Dialog$1.Portal, { keepMounted, container, children: [
    backdrop && /* @__PURE__ */ jsx(Dialog$1.Backdrop, { className: "fui-backdrop" }),
    /* @__PURE__ */ jsxs(
      Dialog$1.Popup,
      {
        className: classes("fui-dialog", className),
        "data-placement": placement,
        "data-size": size === "default" ? void 0 : size,
        "data-padding": padding === "none" ? "none" : void 0,
        "data-close": mode,
        ...props,
        children: [
          /* @__PURE__ */ jsx(
            ChromeContext.Provider,
            {
              value: { close: mode, closeLabel, closeVariant, slotWanted: slotUsers > 0, addSlotUser, slot, setSlot },
              children
            }
          ),
          mode === "corner" && /* @__PURE__ */ jsx(
            Dialog$1.Close,
            {
              render: /* @__PURE__ */ jsx(
                Button,
                {
                  variant: "ghost",
                  size: "icon",
                  className: "fui-dialog-close",
                  "aria-label": closeLabel
                }
              ),
              children: /* @__PURE__ */ jsx(X, { "aria-hidden": true, size: 18 })
            }
          )
        ]
      }
    )
  ] });
}
function DialogTitle({
  className,
  ...props
}) {
  return /* @__PURE__ */ jsx(
    Dialog$1.Title,
    {
      className: classes("fui-dialog-title", className),
      ...props
    }
  );
}
function DialogDescription({
  className,
  ...props
}) {
  return /* @__PURE__ */ jsx(
    Dialog$1.Description,
    {
      className: classes("fui-description", className),
      ...props
    }
  );
}
function DialogHeader({ className, ...props }) {
  return /* @__PURE__ */ jsx("div", { className: classes("fui-dialog-header", className), ...props });
}
function DialogBody({
  className,
  scroll = true,
  padding = "default",
  ...props
}) {
  return /* @__PURE__ */ jsx(
    "div",
    {
      className: classes("fui-dialog-body", className),
      "data-scroll": scroll ? void 0 : "false",
      "data-padding": padding === "none" ? "none" : void 0,
      ...props
    }
  );
}
function DialogFooter({
  className,
  children,
  showCloseButton,
  closeLabel,
  closeVariant,
  ref,
  ...props
}) {
  const chrome = useContext(ChromeContext);
  const close = showCloseButton ?? chrome?.close === "footer";
  const element = useRef(null);
  const setElement = useCallback(
    (node) => {
      element.current = node;
      assignRef(ref, node);
    },
    [ref]
  );
  useLayoutEffect(() => {
    const footer = element.current;
    const dialog = footer?.closest(".fui-dialog");
    if (!footer || !dialog || typeof ResizeObserver === "undefined") return;
    const publish = () => dialog.style.setProperty("--fui-dialog-footer-size", `${footer.getBoundingClientRect().height}px`);
    publish();
    const observer = new ResizeObserver(publish);
    observer.observe(footer);
    return () => {
      observer.disconnect();
      dialog.style.removeProperty("--fui-dialog-footer-size");
    };
  }, []);
  return /* @__PURE__ */ jsxs(
    "div",
    {
      ref: setElement,
      className: classes("fui-dialog-footer", className),
      ...props,
      children: [
        close && /* @__PURE__ */ jsx(Dialog$1.Close, { render: /* @__PURE__ */ jsx(Button, { variant: closeVariant ?? chrome?.closeVariant ?? "secondary" }), children: closeLabel ?? chrome?.closeLabel ?? "Close" }),
        children,
        chrome?.slotWanted && /* @__PURE__ */ jsx("span", { ref: chrome.setSlot, className: "fui-dialog-footer-slot" })
      ]
    }
  );
}
function DialogActions({ children }) {
  const chrome = useContext(ChromeContext);
  const addSlotUser = chrome?.addSlotUser;
  useEffect(() => addSlotUser?.(), [addSlotUser]);
  return chrome?.slot ? createPortal(children, chrome.slot) : null;
}
const SheetContext = createContext(null);
function Sheet({
  modal = true,
  disablePointerDismissal,
  closeOnEscape = "anywhere",
  onOpenChange,
  ...props
}) {
  const popup = useRef(null);
  const scoped = (open, details) => {
    if (!open && closeOnEscape === "focus-inside" && details.reason === "escape-key") {
      const target = details.event.target;
      if (!(target instanceof Node) || !popup.current?.contains(target)) {
        details.cancel();
        return;
      }
    }
    onOpenChange?.(open, details);
  };
  return /* @__PURE__ */ jsx(SheetContext.Provider, { value: { modal: modal === true, popup, closeOnEscape }, children: /* @__PURE__ */ jsx(
    Dialog,
    {
      ...props,
      modal,
      onOpenChange: scoped,
      disablePointerDismissal: disablePointerDismissal ?? (modal === false ? true : void 0)
    }
  ) });
}
const SheetTrigger = DialogTrigger;
const SheetClose = DialogClose;
const SheetHeader = DialogHeader;
const SheetTitle = DialogTitle;
const SheetDescription = DialogDescription;
const SheetFooter = DialogFooter;
const SheetBody = DialogBody;
function SheetContent({
  side = "right",
  ref,
  ...props
}) {
  const sheet = useContext(SheetContext);
  const popup = sheet?.popup;
  const setPopup = useCallback(
    (node) => {
      if (popup) popup.current = node;
      assignRef(ref, node);
    },
    [popup, ref]
  );
  return /* @__PURE__ */ jsx(
    DialogContent,
    {
      ...props,
      ref: setPopup,
      "data-side": side,
      backdrop: sheet?.modal ?? true,
      placement: side === "left" || side === "start" ? "start" : "end"
    }
  );
}
function AlertDialog({
  onOpenChange,
  ...props
}) {
  return /* @__PURE__ */ jsx(AlertDialog$1.Root, { ...props, onOpenChange: guardComposition(onOpenChange) });
}
const AlertDialogTrigger = AlertDialog$1.Trigger;
const AlertDialogClose = AlertDialog$1.Close;
const AlertDialogPortal = AlertDialog$1.Portal;
function AlertDialogOverlay({ className, ...props }) {
  return /* @__PURE__ */ jsx(AlertDialog$1.Backdrop, { className: classes("fui-backdrop", className), ...props });
}
function AlertDialogMedia({ className, ...props }) {
  return /* @__PURE__ */ jsx("div", { "aria-hidden": true, className: classes("fui-alert-dialog-media", className), ...props });
}
const AlertDialogHeader = DialogHeader;
function AlertDialogFooter({ className, ...props }) {
  return /* @__PURE__ */ jsx("div", { className: classes("fui-dialog-footer", className), ...props });
}
const AlertDialogAction = Button;
function AlertDialogCancel({
  variant = "outline",
  size = "default",
  ...props
}) {
  return /* @__PURE__ */ jsx(
    AlertDialog$1.Close,
    {
      render: /* @__PURE__ */ jsx(Button, { variant, size }),
      ...props
    }
  );
}
function AlertDialogContent({
  className,
  ...props
}) {
  return /* @__PURE__ */ jsxs(AlertDialog$1.Portal, { children: [
    /* @__PURE__ */ jsx(AlertDialog$1.Backdrop, { className: "fui-backdrop" }),
    /* @__PURE__ */ jsx(
      AlertDialog$1.Popup,
      {
        className: classes("fui-dialog", className),
        "data-placement": "center",
        ...props
      }
    )
  ] });
}
function AlertDialogTitle({
  className,
  ...props
}) {
  return /* @__PURE__ */ jsx(
    AlertDialog$1.Title,
    {
      className: classes("fui-dialog-title", className),
      ...props
    }
  );
}
function AlertDialogDescription({
  className,
  ...props
}) {
  return /* @__PURE__ */ jsx(
    AlertDialog$1.Description,
    {
      className: classes("fui-description", className),
      ...props
    }
  );
}
export {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogClose,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogMedia,
  AlertDialogOverlay,
  AlertDialogPortal,
  AlertDialogTitle,
  AlertDialogTrigger,
  Dialog,
  DialogActions,
  DialogBody,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogOverlay,
  DialogPortal,
  DialogTitle,
  DialogTrigger,
  Sheet,
  SheetBody,
  SheetClose,
  SheetContent,
  SheetDescription,
  SheetFooter,
  SheetHeader,
  SheetTitle,
  SheetTrigger
};
