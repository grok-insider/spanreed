"use client";
import { jsx } from "react/jsx-runtime";
import { useState, useRef, useEffect } from "react";
import { classes } from "./shared.js";
const UNITS = [
  ["year", 365 * 86400],
  ["month", 30 * 86400],
  ["week", 7 * 86400],
  ["day", 86400],
  ["hour", 3600],
  ["minute", 60],
  ["second", 1]
];
function formatRelativeTime(date, now, locale, opts = {}) {
  const time = new Date(date).getTime();
  if (!Number.isFinite(time)) return "";
  const seconds = Math.round((time - now) / 1e3);
  const abs = Math.abs(seconds);
  const absoluteAfter = (opts.absoluteAfterDays ?? 30) * 86400;
  if (abs >= absoluteAfter) {
    return new Intl.DateTimeFormat(locale, { dateStyle: "medium", timeZone: opts.timeZone }).format(time);
  }
  const format = new Intl.RelativeTimeFormat(locale, { numeric: "auto", style: opts.style ?? "long" });
  if (abs < 45) return format.format(0, "second");
  for (const [unit, size] of UNITS) {
    if (abs >= size || unit === "second") return format.format(Math.round(seconds / size), unit);
  }
  return "";
}
function formatAbsoluteTime(date, locale, format, opts = {}) {
  const value = new Date(date);
  if (!Number.isFinite(value.getTime())) return "";
  const options = typeof format === "function" ? format(value, opts.now ?? Date.now()) : format;
  return new Intl.DateTimeFormat(locale, { timeZone: opts.timeZone, ...options }).format(value);
}
function refreshMs(date, now) {
  const abs = Math.abs(now - new Date(date).getTime());
  if (abs < 6e4) return 5e3;
  if (abs < 36e5) return 3e4;
  return 3e5;
}
function RelativeTime({
  date,
  locale = "en",
  style,
  absoluteAfterDays,
  absoluteFormat,
  now,
  timeZone,
  className,
  title,
  ...props
}) {
  const [tick, setTick] = useState(() => typeof now === "function" ? now() : now ?? Date.now());
  const clock = useRef(now);
  useEffect(() => {
    clock.current = now;
  });
  const fixed = typeof now === "number";
  const live = absoluteFormat === void 0 && !fixed;
  useEffect(() => {
    if (!live) return;
    const read = () => {
      const source = clock.current;
      return typeof source === "function" ? source() : Date.now();
    };
    let timer = 0;
    const schedule = () => {
      timer = window.setTimeout(() => {
        if (document.visibilityState === "visible") setTick(read());
        schedule();
      }, refreshMs(date, read()));
    };
    const onVisible = () => {
      if (document.visibilityState === "visible") setTick(read());
    };
    schedule();
    document.addEventListener("visibilitychange", onVisible);
    return () => {
      window.clearTimeout(timer);
      document.removeEventListener("visibilitychange", onVisible);
    };
  }, [date, live]);
  const current = fixed ? now : tick;
  const time = new Date(date);
  const valid = Number.isFinite(time.getTime());
  const formatZone = typeof absoluteFormat === "function" ? valid ? absoluteFormat(time, current).timeZone : void 0 : absoluteFormat?.timeZone;
  const zone = timeZone ?? formatZone;
  const exact = absoluteFormat !== void 0 || zone !== void 0;
  const text = absoluteFormat !== void 0 ? formatAbsoluteTime(date, locale, absoluteFormat, { now: current, timeZone }) : formatRelativeTime(date, current, locale, { style, absoluteAfterDays, timeZone });
  return /* @__PURE__ */ jsx(
    "time",
    {
      "data-slot": "relative-time",
      "data-mode": live ? "live" : "fixed",
      dateTime: valid ? time.toISOString() : void 0,
      title: title ?? (valid ? new Intl.DateTimeFormat(locale, exact ? { dateStyle: "full", timeStyle: "long", timeZone: zone } : { dateStyle: "medium", timeStyle: "short" }).format(time) : void 0),
      className: classes("fui-relative-time", className),
      suppressHydrationWarning: live || void 0,
      ...props,
      children: text
    }
  );
}
export {
  RelativeTime,
  formatAbsoluteTime,
  formatRelativeTime
};
