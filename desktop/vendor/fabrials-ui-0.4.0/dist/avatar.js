"use client";
import { jsx } from "react/jsx-runtime";
import { Avatar as Avatar$1 } from "@base-ui/react/avatar";
import { classes } from "./shared.js";
function Avatar({
  className,
  size = "md",
  ...props
}) {
  return /* @__PURE__ */ jsx(
    Avatar$1.Root,
    {
      "data-slot": "avatar",
      "data-size": size,
      className: classes("fui-avatar", className),
      ...props
    }
  );
}
function AvatarImage({
  className,
  ...props
}) {
  return /* @__PURE__ */ jsx(
    Avatar$1.Image,
    {
      "data-slot": "avatar-image",
      className: classes("fui-avatar-image", className),
      ...props
    }
  );
}
function AvatarFallback({
  className,
  ...props
}) {
  return /* @__PURE__ */ jsx(
    Avatar$1.Fallback,
    {
      "data-slot": "avatar-fallback",
      className: classes("fui-avatar-fallback", className),
      ...props
    }
  );
}
export {
  Avatar,
  AvatarFallback,
  AvatarImage
};
