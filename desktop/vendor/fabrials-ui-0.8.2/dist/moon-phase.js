"use client";
import { jsxs, jsx, Fragment } from "react/jsx-runtime";
import { useId, useRef, useEffect } from "react";
import { moonVariant, pickMoonVariant, MOON_VARIANTS, moonFrame, MOON_R, MOON_CY, MOON_CX, moonVariantCaption } from "./moon-math.js";
import { classes } from "./shared.js";
const REVEAL_MS = 1200;
function MoonPhase({
  phase = 0.5,
  animate = false,
  cycleMs = 16e3,
  size = 128,
  halo = true,
  variant = "regular",
  caption = false,
  onVariantChange,
  label,
  className,
  style
}) {
  const uid = useId().replace(/:/g, "");
  const groupRef = useRef(null);
  const shadowRef = useRef(null);
  const glowOuterRef = useRef(null);
  const glowInnerRef = useRef(null);
  const tintRef = useRef(null);
  const ringRef = useRef(null);
  const captionRef = useRef(null);
  const onVariantChangeRef = useRef(onVariantChange);
  useEffect(() => {
    onVariantChangeRef.current = onVariantChange;
  }, [onVariantChange]);
  useEffect(() => {
    const fixed = variant === "random" ? null : moonVariant(variant);
    let current = fixed ?? pickMoonVariant();
    const announce = () => {
      if (captionRef.current) captionRef.current.textContent = moonVariantCaption(current);
      onVariantChangeRef.current?.(current);
    };
    const draw = (value, reveal) => {
      const frame = moonFrame(value, current, reveal);
      groupRef.current?.setAttribute("transform", frame.transform);
      shadowRef.current?.setAttribute("d", frame.shadowPath);
      if (shadowRef.current) shadowRef.current.style.fillOpacity = String(frame.shadowOpacity);
      for (const [ref, opacity] of [
        [glowOuterRef, frame.glowOuterOpacity],
        [glowInnerRef, frame.glowInnerOpacity]
      ]) {
        ref.current?.setAttribute("d", frame.litPath);
        ref.current?.setAttribute("fill", frame.glowColor);
        ref.current?.setAttribute("opacity", String(opacity));
      }
      tintRef.current?.setAttribute("fill", frame.tint);
      tintRef.current?.setAttribute("opacity", String(frame.tintOpacity));
      ringRef.current?.setAttribute("opacity", String(frame.ringOpacity));
      if (captionRef.current) captionRef.current.style.opacity = String(frame.captionOpacity);
    };
    announce();
    const reduce = window.matchMedia("(prefers-reduced-motion: reduce)");
    let raf = 0;
    let start = null;
    let cycle = null;
    const revealMs = fixed ? 0 : REVEAL_MS;
    const tick = (now) => {
      start ??= now;
      const elapsed = now - start;
      const value = phase + elapsed / cycleMs;
      const index = Math.floor(value);
      if (!fixed && cycle !== null && index !== cycle) {
        current = pickMoonVariant();
        announce();
      }
      cycle = index;
      draw(value, revealMs ? Math.min(1, elapsed / revealMs) : 1);
      raf = requestAnimationFrame(tick);
    };
    const sync = () => {
      cancelAnimationFrame(raf);
      start = null;
      cycle = null;
      if (animate && !reduce.matches) raf = requestAnimationFrame(tick);
      else draw(phase, 1);
    };
    sync();
    reduce.addEventListener("change", sync);
    return () => {
      cancelAnimationFrame(raf);
      reduce.removeEventListener("change", sync);
    };
  }, [animate, cycleMs, phase, variant]);
  const initialVariant = variant === "random" ? MOON_VARIANTS[0] : moonVariant(variant);
  const initial = moonFrame(phase, initialVariant);
  const id = (name) => `fui-moon-${name}-${uid}`;
  const dimension = typeof size === "number" ? `${size}px` : size;
  return /* @__PURE__ */ jsxs(
    "span",
    {
      "aria-hidden": label ? void 0 : true,
      "aria-label": label,
      className: classes("fui-moon-phase", className),
      role: label ? "img" : void 0,
      style,
      children: [
        /* @__PURE__ */ jsx("span", { className: "fui-moon-phase-box", style: { width: dimension, height: dimension }, children: /* @__PURE__ */ jsxs("svg", { className: "fui-moon-phase-art", viewBox: "0 0 512 512", children: [
          /* @__PURE__ */ jsxs("defs", { children: [
            /* @__PURE__ */ jsxs("radialGradient", { id: id("face"), cx: "0.38", cy: "0.34", r: "0.78", children: [
              /* @__PURE__ */ jsx("stop", { offset: "0", stopColor: "#FBFCFF" }),
              /* @__PURE__ */ jsx("stop", { offset: "0.55", stopColor: "#DCE3EF" }),
              /* @__PURE__ */ jsx("stop", { offset: "1", stopColor: "#A9B5CC" })
            ] }),
            /* @__PURE__ */ jsxs("radialGradient", { id: id("crater"), cx: "0.4", cy: "0.4", r: "0.6", children: [
              /* @__PURE__ */ jsx("stop", { offset: "0", stopColor: "#8A97B2", stopOpacity: "0.9" }),
              /* @__PURE__ */ jsx("stop", { offset: "1", stopColor: "#8A97B2", stopOpacity: "0.35" })
            ] }),
            /* @__PURE__ */ jsx("filter", { id: id("soft"), x: "-20%", y: "-20%", width: "140%", height: "140%", children: /* @__PURE__ */ jsx("feGaussianBlur", { stdDeviation: "6" }) }),
            /* @__PURE__ */ jsx("filter", { id: id("edge"), x: "-10%", y: "-10%", width: "120%", height: "120%", children: /* @__PURE__ */ jsx("feGaussianBlur", { stdDeviation: "4" }) }),
            /* @__PURE__ */ jsx("filter", { id: id("glow-outer"), x: "-80%", y: "-80%", width: "260%", height: "260%", children: /* @__PURE__ */ jsx("feGaussianBlur", { stdDeviation: "58" }) }),
            /* @__PURE__ */ jsx("filter", { id: id("glow-inner"), x: "-40%", y: "-40%", width: "180%", height: "180%", children: /* @__PURE__ */ jsx("feGaussianBlur", { stdDeviation: "18" }) }),
            /* @__PURE__ */ jsx("filter", { id: id("ring"), x: "-20%", y: "-20%", width: "140%", height: "140%", children: /* @__PURE__ */ jsx("feGaussianBlur", { stdDeviation: "3" }) }),
            /* @__PURE__ */ jsx("clipPath", { id: id("disc"), children: /* @__PURE__ */ jsx("circle", { cx: MOON_CX, cy: MOON_CY, r: MOON_R }) }),
            /* @__PURE__ */ jsx("clipPath", { id: id("shadow-clip"), children: /* @__PURE__ */ jsx("circle", { cx: MOON_CX, cy: MOON_CY, r: MOON_R + 1 }) })
          ] }),
          /* @__PURE__ */ jsx(
            "circle",
            {
              ref: ringRef,
              className: "fui-moon-phase-ring",
              cx: MOON_CX,
              cy: MOON_CY,
              fill: "none",
              filter: `url(#${id("ring")})`,
              opacity: initial.ringOpacity,
              r: 238
            }
          ),
          /* @__PURE__ */ jsxs("g", { ref: groupRef, transform: initial.transform, children: [
            halo ? /* @__PURE__ */ jsxs(Fragment, { children: [
              /* @__PURE__ */ jsx(
                "path",
                {
                  ref: glowOuterRef,
                  d: initial.litPath,
                  fill: initial.glowColor,
                  filter: `url(#${id("glow-outer")})`,
                  opacity: initial.glowOuterOpacity
                }
              ),
              /* @__PURE__ */ jsx(
                "path",
                {
                  ref: glowInnerRef,
                  d: initial.litPath,
                  fill: initial.glowColor,
                  filter: `url(#${id("glow-inner")})`,
                  opacity: initial.glowInnerOpacity
                }
              )
            ] }) : null,
            /* @__PURE__ */ jsx("circle", { cx: MOON_CX, cy: MOON_CY, r: MOON_R, fill: `url(#${id("face")})` }),
            /* @__PURE__ */ jsxs("g", { clipPath: `url(#${id("disc")})`, children: [
              /* @__PURE__ */ jsxs("g", { fill: "#8A97B2", fillOpacity: "0.45", filter: `url(#${id("soft")})`, children: [
                /* @__PURE__ */ jsx("ellipse", { cx: "200", cy: "200", rx: "58", ry: "44", transform: "rotate(-20 200 200)" }),
                /* @__PURE__ */ jsx("ellipse", { cx: "292", cy: "176", rx: "42", ry: "30", transform: "rotate(15 292 176)" }),
                /* @__PURE__ */ jsx("ellipse", { cx: "236", cy: "290", rx: "70", ry: "40", transform: "rotate(25 236 290)" }),
                /* @__PURE__ */ jsx("ellipse", { cx: "320", cy: "262", rx: "30", ry: "24" }),
                /* @__PURE__ */ jsx("ellipse", { cx: "166", cy: "304", rx: "26", ry: "20" })
              ] }),
              /* @__PURE__ */ jsxs("g", { stroke: "#F4F7FD", strokeOpacity: "0.7", strokeWidth: "3", children: [
                /* @__PURE__ */ jsx("circle", { cx: "300", cy: "352", r: "22", fill: `url(#${id("crater")})` }),
                /* @__PURE__ */ jsx("circle", { cx: "352", cy: "214", r: "13", fill: `url(#${id("crater")})` }),
                /* @__PURE__ */ jsx("circle", { cx: "150", cy: "232", r: "11", fill: `url(#${id("crater")})` }),
                /* @__PURE__ */ jsx("circle", { cx: "262", cy: "232", r: "8", fill: `url(#${id("crater")})` }),
                /* @__PURE__ */ jsx("circle", { cx: "210", cy: "370", r: "9", fill: `url(#${id("crater")})` })
              ] }),
              /* @__PURE__ */ jsx(
                "circle",
                {
                  cx: MOON_CX,
                  cy: MOON_CY,
                  r: MOON_R,
                  fill: "none",
                  stroke: "#6F7C99",
                  strokeOpacity: "0.45",
                  strokeWidth: "34",
                  transform: "translate(14 14)",
                  filter: `url(#${id("soft")})`
                }
              ),
              /* @__PURE__ */ jsx(
                "circle",
                {
                  ref: tintRef,
                  className: "fui-moon-phase-tint",
                  cx: MOON_CX,
                  cy: MOON_CY,
                  fill: initial.tint,
                  opacity: initial.tintOpacity,
                  r: MOON_R
                }
              )
            ] }),
            /* @__PURE__ */ jsx("circle", { cx: MOON_CX, cy: MOON_CY, r: MOON_R - 1, fill: "none", stroke: "#E9EEF8", strokeOpacity: "0.8", strokeWidth: "3" }),
            /* @__PURE__ */ jsx("g", { clipPath: `url(#${id("shadow-clip")})`, children: /* @__PURE__ */ jsx(
              "path",
              {
                ref: shadowRef,
                className: "fui-moon-phase-shadow",
                d: initial.shadowPath,
                filter: `url(#${id("edge")})`,
                style: { fillOpacity: initial.shadowOpacity }
              }
            ) })
          ] })
        ] }) }),
        caption ? /* @__PURE__ */ jsx("span", { ref: captionRef, "aria-hidden": true, className: "fui-moon-phase-caption", style: { opacity: initial.captionOpacity }, children: moonVariantCaption(initialVariant) }) : null
      ]
    }
  );
}
function Starfield({ twinkle = true, className }) {
  return /* @__PURE__ */ jsxs("span", { "aria-hidden": true, className: classes("fui-starfield", className), children: [
    /* @__PURE__ */ jsx("span", { className: "fui-starfield-near" }),
    /* @__PURE__ */ jsx("span", { className: classes("fui-starfield-far", twinkle && "fui-starfield-twinkle") })
  ] });
}
export {
  MoonPhase,
  Starfield
};
