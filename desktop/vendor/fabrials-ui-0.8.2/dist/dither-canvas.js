"use client";
import { jsx } from "react/jsx-runtime";
import { useRef, useSyncExternalStore, useEffect } from "react";
import { hexToRgb, paintRampField, paintField, seeded } from "./dither.js";
import { DITHER_BACKGROUND } from "./dither-presets.js";
import { classes } from "./shared.js";
function readTheme() {
  const root = document.documentElement;
  return root.classList.contains("dark") || root.dataset.theme === "dark" ? "dark" : "light";
}
function themeOf(element) {
  return element.closest('.dark, [data-theme="dark"]') ? "dark" : element.closest('[data-theme="light"]') ? "light" : readTheme();
}
function cssColor(value) {
  const probe = document.createElement("canvas").getContext("2d", { willReadFrequently: true });
  if (!probe || !value) return void 0;
  probe.fillStyle = "#000";
  probe.fillStyle = value;
  probe.fillRect(0, 0, 1, 1);
  const [r, g, b] = probe.getImageData(0, 0, 1, 1).data;
  return [r, g, b];
}
function resolveRamp(canvas, colors) {
  const style = getComputedStyle(canvas);
  return colors.map((color) => {
    if (color !== DITHER_BACKGROUND) return hexToRgb(color);
    const surface = style.getPropertyValue("--fui-dither-bg").trim() || style.getPropertyValue("--background").trim();
    return cssColor(surface) ?? [22, 26, 33];
  });
}
function subscribeTheme(onChange) {
  const observer = new MutationObserver(onChange);
  observer.observe(document.documentElement, { attributes: true, attributeFilter: ["class", "data-theme"] });
  return () => observer.disconnect();
}
function DitherCanvas({ ramp, field, cell = 2, order = "brightness", reveal, stars, fixed = false, paintKey, className, style }) {
  const ref = useRef(null);
  const fieldRef = useRef(field);
  const theme = useSyncExternalStore(subscribeTheme, readTheme, () => "dark");
  const rampKey = JSON.stringify(ramp);
  const starsKey = JSON.stringify(stars ?? null);
  useEffect(() => {
    fieldRef.current = field;
  }, [field]);
  useEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    const current = themeOf(canvas);
    const colors = resolveRamp(canvas, Array.isArray(ramp) ? ramp : ramp[current]);
    const paint = (progress = 1) => {
      try {
        const rect = canvas.getBoundingClientRect();
        const width = Math.max(1, Math.ceil(rect.width / cell));
        const height = Math.max(1, Math.ceil(rect.height / cell));
        canvas.width = width;
        canvas.height = height;
        const ctx = canvas.getContext("2d", { willReadFrequently: true });
        if (!ctx) return;
        const image = ctx.createImageData(width, height);
        const brightness = fieldRef.current(current, { width, height }, progress);
        if (order === "ramp") paintRampField(image.data, width, height, brightness, colors);
        else paintField(image.data, width, height, brightness, colors);
        ctx.putImageData(image, 0, 0);
        if (stars && current === "dark") {
          const random = seeded(stars.seed ?? 11);
          const palette = stars.colors?.length ? stars.colors : ["#f4f7ff"];
          const count = Math.round(width * height * stars.density / 1e3);
          for (let s = 0; s < count; s++) {
            const x = Math.floor(random() * width);
            const y = Math.floor(Math.pow(random(), 1.4) * height * (stars.reach ?? 1));
            ctx.globalAlpha = 0.25 + random() * 0.6;
            ctx.fillStyle = palette[Math.floor(random() * palette.length)];
            ctx.fillRect(x, y, 1, 1);
          }
          ctx.globalAlpha = 1;
        }
        canvas.dataset.painted = "";
      } catch (error) {
        console.warn("[fabrials-ui] DitherCanvas paint failed", error);
      }
    };
    let frame = 0;
    const animate = reveal && !canvas.dataset.revealed && !matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (animate) {
      canvas.dataset.revealed = "";
      const start = performance.now();
      const step = (now) => {
        const t = Math.min(1, (now - start) / reveal);
        paint(1 - Math.pow(1 - t, 3));
        if (t < 1) frame = requestAnimationFrame(step);
      };
      paint(0);
      frame = requestAnimationFrame(step);
    } else paint();
    let timer = 0;
    let size = `${canvas.clientWidth}x${canvas.clientHeight}`;
    const observer = new ResizeObserver(() => {
      const next = `${canvas.clientWidth}x${canvas.clientHeight}`;
      if (next === size) return;
      size = next;
      window.clearTimeout(timer);
      timer = window.setTimeout(() => {
        cancelAnimationFrame(frame);
        paint();
      }, 120);
    });
    observer.observe(canvas);
    return () => {
      observer.disconnect();
      window.clearTimeout(timer);
      cancelAnimationFrame(frame);
    };
  }, [theme, cell, order, reveal, rampKey, starsKey, paintKey]);
  return /* @__PURE__ */ jsx(
    "canvas",
    {
      ref,
      "aria-hidden": true,
      "data-slot": "dither-canvas",
      "data-fixed": fixed || void 0,
      className: classes("fui-dither-canvas", className),
      style
    }
  );
}
export {
  DitherCanvas
};
