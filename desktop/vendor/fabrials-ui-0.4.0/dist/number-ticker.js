"use client";
import { jsxs, jsx } from "react/jsx-runtime";
import { useInView, useReducedMotion, animate, motion } from "motion/react";
import { useRef, useState, useEffect, useMemo } from "react";
import { classes } from "./shared.js";
const EASE_OUT = [0.16, 1, 0.3, 1];
const DIGIT_HEIGHT_EM = 1.1;
const DIGITS = Array.from({ length: 10 }, (_, n) => n);
function NumberTicker({
  value,
  pad,
  duration = 0.9,
  stagger = 0.04,
  startOnView = true,
  prefix,
  suffix,
  blur = false,
  className,
  digitClassName,
  locale,
  format
}) {
  const containerRef = useRef(null);
  const inView = useInView(containerRef, { once: true, amount: 0.6 });
  const [armed, setArmed] = useState(!startOnView);
  const [entered, setEntered] = useState(false);
  useEffect(() => {
    if (startOnView && inView) setArmed(true);
  }, [startOnView, inView]);
  const text = useMemo(() => {
    if (format) return format(value);
    const rounded = Math.round(value);
    const formatted = locale ? rounded.toLocaleString() : rounded.toString();
    return pad ? formatted.padStart(pad, "0") : formatted;
  }, [value, pad, format, locale]);
  const glyphs = useMemo(
    () => text.split("").map((char, index, chars) => ({ char, id: `g-${chars.length - 1 - index}` })),
    [text]
  );
  const readableText = `${prefix ?? ""}${text}${suffix ?? ""}`;
  useEffect(() => {
    if (!armed || entered) return;
    const total = (duration + glyphs.length * stagger) * 1e3;
    const timer = window.setTimeout(() => setEntered(true), total);
    return () => window.clearTimeout(timer);
  }, [armed, entered, duration, stagger, glyphs.length]);
  return /* @__PURE__ */ jsxs("span", { ref: containerRef, className: classes("fui-ticker", className), children: [
    /* @__PURE__ */ jsx("span", { className: "fui-sr-only", children: readableText }),
    /* @__PURE__ */ jsxs("span", { "aria-hidden": "true", className: "fui-ticker-row", children: [
      prefix ? /* @__PURE__ */ jsx("span", { children: prefix }) : null,
      glyphs.map(({ char, id }, index) => {
        if (!/\d/.test(char)) {
          return /* @__PURE__ */ jsx("span", { className: "fui-ticker-char", children: char }, id);
        }
        return /* @__PURE__ */ jsx(
          Digit,
          {
            digit: armed ? Number(char) : 0,
            delay: entered ? 0 : index * stagger,
            duration,
            blur,
            className: digitClassName
          },
          id
        );
      }),
      suffix ? /* @__PURE__ */ jsx("span", { children: suffix }) : null
    ] })
  ] });
}
function Digit({
  digit,
  delay,
  duration,
  blur,
  className
}) {
  const reduce = useReducedMotion();
  const columnRef = useRef(null);
  useEffect(() => {
    if (reduce || !blur || !columnRef.current || !Number.isFinite(digit)) return;
    const node = columnRef.current;
    const controls = animate(
      node,
      { filter: ["blur(10px)", "blur(0px)"] },
      { duration: Math.min(duration * 0.75, 0.32), delay, ease: EASE_OUT }
    );
    return () => {
      controls.stop();
      node.style.filter = "blur(0px)";
    };
  }, [blur, delay, digit, duration, reduce]);
  if (reduce) {
    return /* @__PURE__ */ jsx("span", { className: classes("fui-ticker-digit", className), style: { width: "1ch" }, children: digit });
  }
  return /* @__PURE__ */ jsx(
    "span",
    {
      className: classes("fui-ticker-digit", className),
      style: { height: `${DIGIT_HEIGHT_EM}em`, width: "1ch" },
      children: /* @__PURE__ */ jsx(
        motion.span,
        {
          ref: columnRef,
          initial: { y: 0 },
          animate: { y: `-${digit * DIGIT_HEIGHT_EM}em` },
          transition: reduce ? { duration: 0 } : { duration, delay, ease: EASE_OUT },
          className: "fui-ticker-column",
          children: DIGITS.map((n) => /* @__PURE__ */ jsx("span", { className: "fui-ticker-glyph", children: n }, n))
        }
      )
    }
  );
}
export {
  NumberTicker
};
