"use client";
import { jsx } from "react/jsx-runtime";
import { classes } from "./shared.js";
const BYTE_UNITS = ["byte", "kilobyte", "megabyte", "gigabyte", "terabyte"];
function isEnglish(locale) {
  try {
    return new Intl.Locale(locale).language === "en";
  } catch {
    return false;
  }
}
function formatBytes(locale, bytes) {
  if (!Number.isFinite(bytes) || bytes < 0) throw new RangeError("Invalid byte count");
  let value = bytes;
  let index = 0;
  while (value >= 1024 && index < BYTE_UNITS.length - 1) {
    value /= 1024;
    index += 1;
  }
  if (index === 0 && isEnglish(locale)) return `${new Intl.NumberFormat(locale, { maximumFractionDigits: 0 }).format(value)} B`;
  return new Intl.NumberFormat(locale, {
    style: "unit",
    unit: BYTE_UNITS[index],
    unitDisplay: "short",
    maximumFractionDigits: index === 0 || value >= 100 ? 0 : 1
  }).format(value);
}
function FileSize({
  bytes,
  locale = "en",
  fallback = null,
  className,
  title,
  ...props
}) {
  if (typeof bytes !== "number" || !Number.isFinite(bytes) || bytes < 0) {
    return fallback === null ? null : /* @__PURE__ */ jsx("span", { className: classes("fui-file-size", className), ...props, children: fallback });
  }
  return /* @__PURE__ */ jsx(
    "span",
    {
      "data-slot": "file-size",
      className: classes("fui-file-size", className),
      title: title ?? new Intl.NumberFormat(locale, { style: "unit", unit: "byte", unitDisplay: "long" }).format(bytes),
      ...props,
      children: formatBytes(locale, bytes)
    }
  );
}
export {
  FileSize,
  formatBytes
};
