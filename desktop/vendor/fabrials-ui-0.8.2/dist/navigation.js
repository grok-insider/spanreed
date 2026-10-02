"use client";
import { jsx, jsxs } from "react/jsx-runtime";
import { mergeProps } from "@base-ui/react/merge-props";
import { useRender } from "@base-ui/react/use-render";
import { Ellipsis, ChevronRight, ChevronLeft } from "lucide-react";
import { classes } from "./shared.js";
function Breadcrumb({
  className,
  "aria-label": label = "Breadcrumb",
  ...props
}) {
  return /* @__PURE__ */ jsx(
    "nav",
    {
      "data-slot": "breadcrumb",
      "aria-label": label,
      className: classes("fui-breadcrumb", className),
      ...props
    }
  );
}
function BreadcrumbList({ className, ...props }) {
  return /* @__PURE__ */ jsx("ol", { className: classes("fui-breadcrumb-list", className), ...props });
}
function BreadcrumbItem({ className, ...props }) {
  return /* @__PURE__ */ jsx("li", { className: classes("fui-breadcrumb-item", className), ...props });
}
function BreadcrumbLink({
  className,
  render,
  ...props
}) {
  return useRender({
    defaultTagName: "a",
    props: mergeProps({ className: classes("fui-breadcrumb-link", className) }, props),
    render
  });
}
function BreadcrumbPage({ className, ...props }) {
  return /* @__PURE__ */ jsx(
    "span",
    {
      "aria-current": "page",
      className: classes("fui-breadcrumb-page", className),
      ...props
    }
  );
}
function BreadcrumbSeparator({
  className,
  children,
  ...props
}) {
  return /* @__PURE__ */ jsx(
    "li",
    {
      role: "presentation",
      "aria-hidden": "true",
      className: classes("fui-breadcrumb-separator", className),
      ...props,
      children: children ?? /* @__PURE__ */ jsx(ChevronRight, {})
    }
  );
}
function BreadcrumbEllipsis({
  className,
  label = "More pages",
  ...props
}) {
  return /* @__PURE__ */ jsxs("span", { className: classes("fui-breadcrumb-ellipsis", className), ...props, children: [
    /* @__PURE__ */ jsx(Ellipsis, { "aria-hidden": true }),
    /* @__PURE__ */ jsx("span", { className: "fui-sr-only", children: label })
  ] });
}
function Pagination({
  className,
  "aria-label": label = "Pagination",
  ...props
}) {
  return /* @__PURE__ */ jsx(
    "nav",
    {
      "data-slot": "pagination",
      "aria-label": label,
      className: classes("fui-pagination", className),
      ...props
    }
  );
}
function PaginationContent({ className, ...props }) {
  return /* @__PURE__ */ jsx("ul", { className: classes("fui-pagination-content", className), ...props });
}
function PaginationItem(props) {
  return /* @__PURE__ */ jsx("li", { ...props });
}
function PaginationLink({
  className,
  isActive = false,
  render,
  ...props
}) {
  return useRender({
    defaultTagName: "a",
    props: mergeProps(
      {
        className: classes("fui-pagination-link", className),
        "aria-current": isActive ? "page" : void 0
      },
      props
    ),
    render
  });
}
function PaginationPrevious({
  className,
  label = "Previous",
  ...props
}) {
  return /* @__PURE__ */ jsxs(
    PaginationLink,
    {
      className: classes("fui-pagination-step", className),
      ...props,
      children: [
        /* @__PURE__ */ jsx(ChevronLeft, { "aria-hidden": true }),
        /* @__PURE__ */ jsx("span", { children: label })
      ]
    }
  );
}
function PaginationNext({
  className,
  label = "Next",
  ...props
}) {
  return /* @__PURE__ */ jsxs(
    PaginationLink,
    {
      className: classes("fui-pagination-step", className),
      ...props,
      children: [
        /* @__PURE__ */ jsx("span", { children: label }),
        /* @__PURE__ */ jsx(ChevronRight, { "aria-hidden": true })
      ]
    }
  );
}
function PaginationEllipsis({
  className,
  label = "More pages",
  ...props
}) {
  return /* @__PURE__ */ jsxs("span", { className: classes("fui-pagination-ellipsis", className), ...props, children: [
    /* @__PURE__ */ jsx(Ellipsis, { "aria-hidden": true }),
    /* @__PURE__ */ jsx("span", { className: "fui-sr-only", children: label })
  ] });
}
export {
  Breadcrumb,
  BreadcrumbEllipsis,
  BreadcrumbItem,
  BreadcrumbLink,
  BreadcrumbList,
  BreadcrumbPage,
  BreadcrumbSeparator,
  Pagination,
  PaginationContent,
  PaginationEllipsis,
  PaginationItem,
  PaginationLink,
  PaginationNext,
  PaginationPrevious
};
