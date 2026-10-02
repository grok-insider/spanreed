"use client";
import { jsxs, jsx } from "react/jsx-runtime";
import { Spinner } from "@fabrials/ui";
import { XIcon, AlertIcon, VideoIcon, PaperclipIcon, ImageIcon, FileTextIcon, MusicIcon } from "./chat-icons.js";
function attachmentCategory(mediaType) {
  const type = mediaType ?? "";
  if (type.startsWith("image/")) return "image";
  if (type.startsWith("video/")) return "video";
  if (type.startsWith("audio/")) return "audio";
  if (type.startsWith("application/") || type.startsWith("text/")) return "document";
  return "unknown";
}
function formatBytes(bytes) {
  if (bytes === void 0 || !Number.isFinite(bytes) || bytes < 0) return "";
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value >= 10 ? Math.round(value) : Math.round(value * 10) / 10} ${units[unit]}`;
}
function typeLabel(mediaType) {
  const subtype = mediaType.split(";")[0]?.split("/")[1] ?? "";
  return subtype ? subtype.replace(/^x-/, "").replace(/^vnd\..*\./, "").toUpperCase() : "";
}
function attachmentDetail(item) {
  return [typeLabel(item.mediaType), formatBytes(item.size)].filter(Boolean).join(" · ");
}
const ICONS = {
  audio: MusicIcon,
  document: FileTextIcon,
  image: ImageIcon,
  unknown: PaperclipIcon,
  video: VideoIcon
};
function Preview({ item, variant }) {
  const category = attachmentCategory(item.mediaType);
  const Icon = item.status === "error" ? AlertIcon : ICONS[category];
  let content = /* @__PURE__ */ jsx(Icon, {});
  if (item.status !== "error" && item.url && category === "image") {
    content = /* @__PURE__ */ jsx("img", { alt: variant === "grid" ? item.name : "", decoding: "async", loading: "lazy", src: item.url });
  } else if (item.status !== "error" && item.url && category === "video") {
    content = /* @__PURE__ */ jsx("video", { "aria-hidden": true, muted: true, playsInline: true, preload: "metadata", src: item.url });
  }
  return /* @__PURE__ */ jsxs("span", { className: "fui-attachment-preview", "data-category": category, children: [
    content,
    item.status === "uploading" ? /* @__PURE__ */ jsx("span", { className: "fui-attachment-busy", children: /* @__PURE__ */ jsx(Spinner, { label: `Uploading ${item.name}` }) }) : null
  ] });
}
function AttachmentChip({
  item,
  variant = "inline",
  onRemove,
  removeLabel = (value) => `Remove ${value.name}`,
  className
}) {
  const detail = item.status === "error" ? item.error || "Upload failed" : item.status === "uploading" ? "Uploading…" : attachmentDetail(item);
  return /* @__PURE__ */ jsxs(
    "div",
    {
      "aria-busy": item.status === "uploading" || void 0,
      className: ["fui-attachment", className].filter(Boolean).join(" "),
      "data-status": item.status ?? "ready",
      "data-variant": variant,
      title: variant === "grid" ? `${item.name}${detail ? ` · ${detail}` : ""}` : void 0,
      children: [
        /* @__PURE__ */ jsx(Preview, { item, variant }),
        variant === "grid" ? item.status === "error" ? /* @__PURE__ */ jsx("span", { className: "fui-sr-only", children: `${item.name}: ${detail}` }) : null : /* @__PURE__ */ jsxs("span", { className: "fui-attachment-info", children: [
          /* @__PURE__ */ jsx("span", { className: "fui-attachment-name", children: item.name }),
          detail ? /* @__PURE__ */ jsx("span", { className: "fui-attachment-detail", role: item.status === "error" ? "alert" : void 0, children: detail }) : null
        ] }),
        onRemove ? /* @__PURE__ */ jsx(
          "button",
          {
            "aria-label": removeLabel(item),
            className: "fui-attachment-remove",
            onClick: (event) => {
              event.stopPropagation();
              onRemove(item.id);
            },
            title: removeLabel(item),
            type: "button",
            children: /* @__PURE__ */ jsx(XIcon, {})
          }
        ) : null
      ]
    }
  );
}
function Attachments({
  items,
  variant = "inline",
  onRemove,
  removeLabel,
  empty = null,
  label = "Attachments",
  className
}) {
  if (items.length === 0) {
    return empty ? /* @__PURE__ */ jsx("div", { className: "fui-attachments-empty", children: empty }) : null;
  }
  return /* @__PURE__ */ jsx("ul", { "aria-label": label, className: ["fui-attachments", className].filter(Boolean).join(" "), "data-variant": variant, children: items.map((item) => /* @__PURE__ */ jsx("li", { children: /* @__PURE__ */ jsx(AttachmentChip, { item, onRemove, removeLabel, variant }) }, item.id)) });
}
export {
  AttachmentChip,
  Attachments,
  attachmentCategory,
  attachmentDetail,
  formatBytes
};
