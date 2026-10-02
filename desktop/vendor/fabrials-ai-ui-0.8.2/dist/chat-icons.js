"use client";
import { jsx, jsxs, Fragment } from "react/jsx-runtime";
function icon(paths) {
  return function Icon({ className, ...props }) {
    return /* @__PURE__ */ jsx(
      "svg",
      {
        "aria-hidden": "true",
        className,
        fill: "none",
        height: "16",
        stroke: "currentColor",
        strokeLinecap: "round",
        strokeLinejoin: "round",
        strokeWidth: "2",
        viewBox: "0 0 24 24",
        width: "16",
        xmlns: "http://www.w3.org/2000/svg",
        ...props,
        children: paths
      }
    );
  };
}
const ChevronDownIcon = icon(/* @__PURE__ */ jsx("path", { d: "m6 9 6 6 6-6" }));
const XIcon = icon(
  /* @__PURE__ */ jsxs(Fragment, { children: [
    /* @__PURE__ */ jsx("path", { d: "M18 6 6 18" }),
    /* @__PURE__ */ jsx("path", { d: "m6 6 12 12" })
  ] })
);
const CopyIcon = icon(
  /* @__PURE__ */ jsxs(Fragment, { children: [
    /* @__PURE__ */ jsx("rect", { height: "14", rx: "2", ry: "2", width: "14", x: "8", y: "8" }),
    /* @__PURE__ */ jsx("path", { d: "M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2" })
  ] })
);
const CheckIcon = icon(/* @__PURE__ */ jsx("path", { d: "M20 6 9 17l-5-5" }));
const DownloadIcon = icon(
  /* @__PURE__ */ jsxs(Fragment, { children: [
    /* @__PURE__ */ jsx("path", { d: "M12 15V3" }),
    /* @__PURE__ */ jsx("path", { d: "M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" }),
    /* @__PURE__ */ jsx("path", { d: "m7 10 5 5 5-5" })
  ] })
);
const MicIcon = icon(
  /* @__PURE__ */ jsxs(Fragment, { children: [
    /* @__PURE__ */ jsx("path", { d: "M12 19v3" }),
    /* @__PURE__ */ jsx("path", { d: "M19 10v2a7 7 0 0 1-14 0v-2" }),
    /* @__PURE__ */ jsx("rect", { height: "13", rx: "3", width: "6", x: "9", y: "2" })
  ] })
);
const ArrowUpIcon = icon(
  /* @__PURE__ */ jsxs(Fragment, { children: [
    /* @__PURE__ */ jsx("path", { d: "m5 12 7-7 7 7" }),
    /* @__PURE__ */ jsx("path", { d: "M12 19V5" })
  ] })
);
const SquareIcon = icon(
  /* @__PURE__ */ jsx("rect", { fill: "currentColor", height: "18", rx: "2", width: "18", x: "3", y: "3" })
);
const FileTextIcon = icon(
  /* @__PURE__ */ jsxs(Fragment, { children: [
    /* @__PURE__ */ jsx("path", { d: "M6 22a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.704.706l3.588 3.588A2.4 2.4 0 0 1 20 8v12a2 2 0 0 1-2 2z" }),
    /* @__PURE__ */ jsx("path", { d: "M14 2v5a1 1 0 0 0 1 1h5" }),
    /* @__PURE__ */ jsx("path", { d: "M10 9H8" }),
    /* @__PURE__ */ jsx("path", { d: "M16 13H8" }),
    /* @__PURE__ */ jsx("path", { d: "M16 17H8" })
  ] })
);
const ImageIcon = icon(
  /* @__PURE__ */ jsxs(Fragment, { children: [
    /* @__PURE__ */ jsx("rect", { height: "18", rx: "2", ry: "2", width: "18", x: "3", y: "3" }),
    /* @__PURE__ */ jsx("circle", { cx: "9", cy: "9", r: "2" }),
    /* @__PURE__ */ jsx("path", { d: "m21 15-3.086-3.086a2 2 0 0 0-2.828 0L6 21" })
  ] })
);
const GlobeIcon = icon(
  /* @__PURE__ */ jsxs(Fragment, { children: [
    /* @__PURE__ */ jsx("circle", { cx: "12", cy: "12", r: "10" }),
    /* @__PURE__ */ jsx("path", { d: "M12 2a14.5 14.5 0 0 0 0 20 14.5 14.5 0 0 0 0-20" }),
    /* @__PURE__ */ jsx("path", { d: "M2 12h20" })
  ] })
);
const SearchIcon = icon(
  /* @__PURE__ */ jsxs(Fragment, { children: [
    /* @__PURE__ */ jsx("path", { d: "m21 21-4.34-4.34" }),
    /* @__PURE__ */ jsx("circle", { cx: "11", cy: "11", r: "8" })
  ] })
);
const BrainIcon = icon(
  /* @__PURE__ */ jsxs(Fragment, { children: [
    /* @__PURE__ */ jsx("path", { d: "M12 18V5" }),
    /* @__PURE__ */ jsx("path", { d: "M15 13a4.17 4.17 0 0 1-3-4 4.17 4.17 0 0 1-3 4" }),
    /* @__PURE__ */ jsx("path", { d: "M17.598 6.5A3 3 0 1 0 12 5a3 3 0 1 0-5.598 1.5" }),
    /* @__PURE__ */ jsx("path", { d: "M17.997 5.125a4 4 0 0 1 2.526 5.77" }),
    /* @__PURE__ */ jsx("path", { d: "M18 18a4 4 0 0 0 2-7.464" }),
    /* @__PURE__ */ jsx("path", { d: "M19.967 17.483A4 4 0 1 1 12 18a4 4 0 1 1-7.967-.517" }),
    /* @__PURE__ */ jsx("path", { d: "M6 18a4 4 0 0 1-2-7.464" }),
    /* @__PURE__ */ jsx("path", { d: "M6.003 5.125a4 4 0 0 0-2.526 5.77" })
  ] })
);
const PaperclipIcon = icon(
  /* @__PURE__ */ jsx("path", { d: "m16 6-8.414 8.586a2 2 0 0 0 2.829 2.829l8.414-8.586a4 4 0 1 0-5.657-5.657l-8.379 8.551a6 6 0 1 0 8.485 8.485l8.379-8.551" })
);
const VideoIcon = icon(
  /* @__PURE__ */ jsxs(Fragment, { children: [
    /* @__PURE__ */ jsx("path", { d: "m16 13 5.223 3.482a.5.5 0 0 0 .777-.416V7.87a.5.5 0 0 0-.752-.432L16 10.5" }),
    /* @__PURE__ */ jsx("rect", { height: "12", rx: "2", width: "14", x: "2", y: "6" })
  ] })
);
const MusicIcon = icon(
  /* @__PURE__ */ jsxs(Fragment, { children: [
    /* @__PURE__ */ jsx("circle", { cx: "8", cy: "18", r: "4" }),
    /* @__PURE__ */ jsx("path", { d: "M12 18V2l7 4" })
  ] })
);
const AlertIcon = icon(
  /* @__PURE__ */ jsxs(Fragment, { children: [
    /* @__PURE__ */ jsx("circle", { cx: "12", cy: "12", r: "10" }),
    /* @__PURE__ */ jsx("line", { x1: "12", x2: "12", y1: "8", y2: "12" }),
    /* @__PURE__ */ jsx("line", { x1: "12", x2: "12.01", y1: "16", y2: "16" })
  ] })
);
const ExternalLinkIcon = icon(
  /* @__PURE__ */ jsxs(Fragment, { children: [
    /* @__PURE__ */ jsx("path", { d: "M15 3h6v6" }),
    /* @__PURE__ */ jsx("path", { d: "M10 14 21 3" }),
    /* @__PURE__ */ jsx("path", { d: "M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6" })
  ] })
);
export {
  AlertIcon,
  ArrowUpIcon,
  BrainIcon,
  CheckIcon,
  ChevronDownIcon,
  CopyIcon,
  DownloadIcon,
  ExternalLinkIcon,
  FileTextIcon,
  GlobeIcon,
  ImageIcon,
  MicIcon,
  MusicIcon,
  PaperclipIcon,
  SearchIcon,
  SquareIcon,
  VideoIcon,
  XIcon
};
