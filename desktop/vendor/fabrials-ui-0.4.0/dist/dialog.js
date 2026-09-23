"use client";
import { jsx, jsxs } from "react/jsx-runtime";
import { Dialog as Dialog$1 } from "@base-ui/react/dialog";
import { AlertDialog as AlertDialog$1 } from "@base-ui/react/alert-dialog";
import { X } from "lucide-react";
import { Button } from "./controls.js";
import { classes } from "./shared.js";
const Dialog = Dialog$1.Root;
const DialogTrigger = Dialog$1.Trigger;
const DialogClose = Dialog$1.Close;
function DialogContent({
  className,
  children,
  closeLabel = "Close",
  showCloseButton = true,
  placement = "center",
  ...props
}) {
  return /* @__PURE__ */ jsxs(Dialog$1.Portal, { children: [
    /* @__PURE__ */ jsx(Dialog$1.Backdrop, { className: "fui-backdrop" }),
    /* @__PURE__ */ jsxs(
      Dialog$1.Popup,
      {
        className: classes("fui-dialog", className),
        "data-placement": placement,
        ...props,
        children: [
          children,
          showCloseButton && /* @__PURE__ */ jsx(
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
function DialogFooter({ className, ...props }) {
  return /* @__PURE__ */ jsx("div", { className: classes("fui-dialog-footer", className), ...props });
}
const Sheet = Dialog;
const SheetTrigger = DialogTrigger;
const SheetClose = DialogClose;
const SheetHeader = DialogHeader;
const SheetTitle = DialogTitle;
const SheetDescription = DialogDescription;
function SheetContent({
  side = "right",
  ...props
}) {
  return /* @__PURE__ */ jsx(
    DialogContent,
    {
      ...props,
      "data-side": side,
      placement: side === "left" ? "start" : "end"
    }
  );
}
const AlertDialog = AlertDialog$1.Root;
const AlertDialogTrigger = AlertDialog$1.Trigger;
const AlertDialogClose = AlertDialog$1.Close;
const AlertDialogHeader = DialogHeader;
const AlertDialogFooter = DialogFooter;
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
  AlertDialogTitle,
  AlertDialogTrigger,
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
  Sheet,
  SheetClose,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
  SheetTrigger
};
