"use client";
import { jsx, jsxs } from "react/jsx-runtime";
import { useId } from "react";
import { classes } from "./shared.js";
function SettingsSection({
  id,
  title,
  description,
  status,
  headingLevel = 2,
  headingRef,
  headingProps,
  layout = "auto",
  className,
  children,
  ...props
}) {
  const generated = useId();
  const headingId = headingProps?.id ?? `${id ?? generated}-heading`;
  const descriptionId = description ? `${id ?? generated}-description` : void 0;
  const Heading = `h${headingLevel}`;
  return /* @__PURE__ */ jsx(
    "section",
    {
      id,
      "aria-labelledby": headingId,
      "aria-describedby": descriptionId,
      "data-layout": layout,
      className: classes("fui-settings-section", className),
      ...props,
      children: /* @__PURE__ */ jsxs("div", { className: "fui-settings-section-layout", children: [
        /* @__PURE__ */ jsxs("div", { className: "fui-settings-section-intro", children: [
          /* @__PURE__ */ jsxs("div", { className: "fui-settings-section-heading", children: [
            /* @__PURE__ */ jsx(Heading, { ...headingProps, id: headingId, ref: headingRef, className: "fui-settings-section-title", children: title }),
            status
          ] }),
          description ? /* @__PURE__ */ jsx("p", { id: descriptionId, className: "fui-settings-section-description", children: description }) : null
        ] }),
        /* @__PURE__ */ jsx("div", { className: "fui-settings-section-body", children })
      ] })
    }
  );
}
export {
  SettingsSection
};
