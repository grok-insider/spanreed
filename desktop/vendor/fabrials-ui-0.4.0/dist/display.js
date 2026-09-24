"use client";
import { jsx, jsxs } from "react/jsx-runtime";
import { Tabs as Tabs$1 } from "@base-ui/react/tabs";
import { classes } from "./shared.js";
function Badge({
  className,
  tone = "neutral",
  variant = "soft",
  dot = false,
  children,
  ...props
}) {
  return /* @__PURE__ */ jsxs(
    "span",
    {
      "data-slot": "badge",
      className: classes("fui-badge", className),
      "data-tone": tone === "accent" ? "info" : tone,
      "data-variant": variant,
      ...props,
      children: [
        dot ? /* @__PURE__ */ jsx("span", { "aria-hidden": true, className: "fui-badge-dot" }) : null,
        children
      ]
    }
  );
}
function Card({
  className,
  interactive = false,
  ...props
}) {
  return /* @__PURE__ */ jsx(
    "section",
    {
      "data-slot": "card",
      "data-interactive": interactive || void 0,
      className: classes("fui-card", className),
      ...props
    }
  );
}
function CardHeader({ className, ...props }) {
  return /* @__PURE__ */ jsx(
    "div",
    {
      "data-slot": "card-header",
      className: classes("fui-card-header", className),
      ...props
    }
  );
}
function CardTitle({
  className,
  as: Heading = "h2",
  ...props
}) {
  return /* @__PURE__ */ jsx(
    Heading,
    {
      "data-slot": "card-title",
      className: classes("fui-card-title", className),
      ...props
    }
  );
}
function CardDescription({ className, ...props }) {
  return /* @__PURE__ */ jsx(
    "p",
    {
      "data-slot": "card-description",
      className: classes("fui-description", className),
      ...props
    }
  );
}
function CardAction({ className, ...props }) {
  return /* @__PURE__ */ jsx(
    "div",
    {
      "data-slot": "card-action",
      className: classes("fui-card-action", className),
      ...props
    }
  );
}
function CardContent({ className, ...props }) {
  return /* @__PURE__ */ jsx(
    "div",
    {
      "data-slot": "card-content",
      className: classes("fui-card-content", className),
      ...props
    }
  );
}
function CardFooter({ className, ...props }) {
  return /* @__PURE__ */ jsx(
    "div",
    {
      "data-slot": "card-footer",
      className: classes("fui-card-footer", className),
      ...props
    }
  );
}
function Alert({
  className,
  variant = "default",
  ...props
}) {
  return /* @__PURE__ */ jsx(
    "div",
    {
      role: variant === "destructive" ? "alert" : "status",
      "data-slot": "alert",
      "data-variant": variant,
      className: classes("fui-alert", className),
      ...props
    }
  );
}
function AlertTitle({ className, ...props }) {
  return /* @__PURE__ */ jsx("div", { className: classes("fui-alert-title", className), ...props });
}
function AlertDescription({
  className,
  ...props
}) {
  return /* @__PURE__ */ jsx(
    "div",
    {
      className: classes("fui-description", "fui-alert-description", className),
      ...props
    }
  );
}
function AlertAction({ className, ...props }) {
  return /* @__PURE__ */ jsx(
    "div",
    {
      className: classes("fui-actions", "fui-alert-action", className),
      ...props
    }
  );
}
function Separator({
  className,
  orientation = "horizontal",
  ...props
}) {
  return /* @__PURE__ */ jsx(
    "hr",
    {
      "data-orientation": orientation,
      "aria-orientation": orientation === "vertical" ? "vertical" : void 0,
      className: classes("fui-separator", className),
      ...props
    }
  );
}
function Skeleton({ className, ...props }) {
  return /* @__PURE__ */ jsx(
    "div",
    {
      "aria-hidden": "true",
      className: classes("fui-skeleton", className),
      ...props
    }
  );
}
function Progress({
  className,
  tone = "neutral",
  ...props
}) {
  return /* @__PURE__ */ jsx(
    "progress",
    {
      "data-tone": tone,
      className: classes("fui-progress", className),
      ...props
    }
  );
}
function Table({
  className,
  children,
  regionLabel,
  stickyHeader = false,
  ...props
}) {
  return /* @__PURE__ */ jsx(
    "div",
    {
      className: "fui-table-scroll",
      "data-sticky-header": stickyHeader || void 0,
      tabIndex: 0,
      role: "region",
      "aria-label": regionLabel ?? (typeof props["aria-label"] === "string" ? `${props["aria-label"]} table` : "Scrollable table"),
      children: /* @__PURE__ */ jsx("table", { className: classes("fui-table", className), ...props, children })
    }
  );
}
const Tabs = Tabs$1.Root;
function TableHeader(props) {
  return /* @__PURE__ */ jsx("thead", { ...props });
}
function TableBody(props) {
  return /* @__PURE__ */ jsx("tbody", { ...props });
}
function TableFooter(props) {
  return /* @__PURE__ */ jsx("tfoot", { ...props });
}
function TableRow(props) {
  return /* @__PURE__ */ jsx("tr", { ...props });
}
function TableHead({
  numeric,
  ...props
}) {
  return /* @__PURE__ */ jsx("th", { scope: "col", "data-numeric": numeric || void 0, ...props });
}
function TableCell({
  numeric,
  ...props
}) {
  return /* @__PURE__ */ jsx("td", { "data-numeric": numeric || void 0, ...props });
}
function TableCaption(props) {
  return /* @__PURE__ */ jsx("caption", { ...props });
}
function TabsList({
  className,
  variant = "underline",
  ...props
}) {
  return /* @__PURE__ */ jsxs(
    Tabs$1.List,
    {
      "data-variant": variant,
      className: classes("fui-tabs-list", className),
      ...props,
      children: [
        props.children,
        variant === "segmented" ? null : /* @__PURE__ */ jsx(Tabs$1.Indicator, { className: "fui-tabs-indicator" })
      ]
    }
  );
}
function TabsTrigger({
  className,
  ...props
}) {
  return /* @__PURE__ */ jsx(Tabs$1.Tab, { className: classes("fui-tab", className), ...props });
}
function TabsContent({
  className,
  ...props
}) {
  return /* @__PURE__ */ jsx(
    Tabs$1.Panel,
    {
      className: classes("fui-tab-panel", className),
      ...props
    }
  );
}
export {
  Alert,
  AlertAction,
  AlertDescription,
  AlertTitle,
  Badge,
  Card,
  CardAction,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
  Progress,
  Separator,
  Skeleton,
  Table,
  TableBody,
  TableCaption,
  TableCell,
  TableFooter,
  TableHead,
  TableHeader,
  TableRow,
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger
};
