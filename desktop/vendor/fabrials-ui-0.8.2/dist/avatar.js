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
const segmenter = typeof Intl !== "undefined" && "Segmenter" in Intl ? new Intl.Segmenter(void 0, { granularity: "grapheme" }) : null;
function firstGrapheme(word) {
  if (segmenter) for (const part of segmenter.segment(word)) return part.segment;
  return Array.from(word)[0] ?? "";
}
function avatarInitials(value, fallback = "?", locale) {
  const text = value.trim();
  if (!text) return fallback;
  const words = /\s/u.test(text) ? text.split(/\s+/u) : [text.split("@")[0] || text];
  const letters = words.length > 1 ? [firstGrapheme(words[0]), firstGrapheme(words[words.length - 1])] : [firstGrapheme(words[0])];
  return letters.join("").toLocaleUpperCase(locale) || fallback;
}
export {
  Avatar,
  AvatarFallback,
  AvatarImage,
  avatarInitials
};
