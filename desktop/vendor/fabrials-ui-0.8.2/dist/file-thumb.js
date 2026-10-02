"use client";
import { jsx, jsxs } from "react/jsx-runtime";
import { useState, useCallback } from "react";
import { FileText } from "lucide-react";
import { classes } from "./shared.js";
function fileTypeLabel(filename, mime) {
  const fromName = filename?.match(/\.([a-z0-9]{1,5})$/i)?.[1];
  const fromMime = mime?.split("/")[1]?.split(/[+;.-]/)[0];
  return (fromName || fromMime || "file").slice(0, 4).toUpperCase();
}
function FileThumb({
  src,
  mime,
  filename,
  alt = "",
  icon,
  size = 40,
  className
}) {
  const [failedSrc, setFailedSrc] = useState(null);
  const checkLoaded = useCallback(
    (img) => {
      if (img && src && img.complete && img.naturalWidth === 0) setFailedSrc(src);
    },
    [src]
  );
  const style = { "--fui-file-thumb-size": `${size}px` };
  const isImage = !mime || mime.startsWith("image/");
  if (src && isImage && failedSrc !== src) {
    return /* @__PURE__ */ jsx(
      "img",
      {
        src,
        alt,
        width: size,
        height: size,
        loading: "lazy",
        decoding: "async",
        className: classes("fui-file-thumb", "fui-file-thumb-image", className),
        style,
        onError: () => setFailedSrc(src),
        ref: checkLoaded
      }
    );
  }
  return /* @__PURE__ */ jsxs(
    "span",
    {
      className: classes("fui-file-thumb", "fui-file-thumb-tile", className),
      style,
      ...alt ? { role: "img", "aria-label": alt } : { "aria-hidden": true },
      children: [
        /* @__PURE__ */ jsx("span", { "aria-hidden": true, className: "fui-file-thumb-icon", children: icon ?? /* @__PURE__ */ jsx(FileText, {}) }),
        /* @__PURE__ */ jsx("span", { "aria-hidden": true, className: "fui-file-thumb-ext", children: fileTypeLabel(filename, mime) })
      ]
    }
  );
}
export {
  FileThumb,
  fileTypeLabel
};
