"use client";
import { jsx } from "react/jsx-runtime";
import { Tabs as Tabs$1 } from "@base-ui/react/tabs";
import { classes } from "./shared.js";
function Badge({
  className,
  tone = "neutral",
  ...props
}) {
  return /* @__PURE__ */ jsx(
    "span",
    {
      className: classes("fui-badge", className),
      "data-tone": tone,
      ...props
    }
  );
}
function Card({ className, ...props }) {
  return /* @__PURE__ */ jsx(
    "section",
    {
      "data-slot": "card",
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
  return /* @__PURE__ */ jsx("div", { className: classes("fui-description", className), ...props });
}
function AlertAction({ className, ...props }) {
  return /* @__PURE__ */ jsx("div", { className: classes("fui-actions", className), ...props });
}
function Separator({ className, ...props }) {
  return /* @__PURE__ */ jsx("hr", { className: classes("fui-separator", className), ...props });
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
function Progress({ className, ...props }) {
  return /* @__PURE__ */ jsx("progress", { className: classes("fui-progress", className), ...props });
}
function Table({
  className,
  children,
  regionLabel,
  ...props
}) {
  return /* @__PURE__ */ jsx(
    "div",
    {
      className: "fui-table-scroll",
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
function TableHead(props) {
  return /* @__PURE__ */ jsx("th", { scope: "col", ...props });
}
function TableCell(props) {
  return /* @__PURE__ */ jsx("td", { ...props });
}
function TableCaption(props) {
  return /* @__PURE__ */ jsx("caption", { ...props });
}
function TabsList({
  className,
  ...props
}) {
  return /* @__PURE__ */ jsx(Tabs$1.List, { className: classes("fui-tabs-list", className), ...props });
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
