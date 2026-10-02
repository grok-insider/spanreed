"use client";
import { jsx } from "react/jsx-runtime";
import { DitherCanvas } from "./dither-canvas.js";
import { STORM_RAMP, gemRamp, stormBandField, gemField, stormField } from "./dither-presets.js";
import { classes } from "./shared.js";
const FRONTS = { hero: [0.12, 0.62], full: [-0.05, 0.7] };
function DitherScene({ variant = "hero", seed, reveal = 1200, cell = 3, className, ...props }) {
  const front = FRONTS[variant];
  const field = (_theme, { width, height }, progress) => stormField(width / height, progress, { front: [front[0], front[1]], seed: seed ?? (variant === "hero" ? 7 : 21) });
  return /* @__PURE__ */ jsx("div", { "aria-hidden": true, "data-slot": "dither-scene", "data-variant": variant, className: classes("fui-dither-scene", className), ...props, children: /* @__PURE__ */ jsx(DitherCanvas, { ramp: STORM_RAMP, order: "ramp", field, cell, reveal: reveal || void 0, paintKey: `${variant}:${seed ?? ""}` }) });
}
function DitherBand({ seed = 33, cell = 3, className, ...props }) {
  const field = (_theme, { width, height }) => stormBandField(width / height, seed);
  return /* @__PURE__ */ jsx("div", { "aria-hidden": true, "data-slot": "dither-band", className: classes("fui-dither-band", className), ...props, children: /* @__PURE__ */ jsx(DitherCanvas, { ramp: STORM_RAMP, order: "ramp", field, cell, paintKey: seed }) });
}
function DitherGem({ gem = "stormlight", size = 20, cell, reveal, className, style, ...props }) {
  const field = (_theme, { width, height }, progress) => gemField(width, height, progress);
  return /* @__PURE__ */ jsx(
    "span",
    {
      "aria-hidden": true,
      "data-slot": "dither-gem",
      "data-gem": gem,
      className: classes("fui-dither-gem", className),
      style: { width: size, height: size, ...style },
      ...props,
      children: /* @__PURE__ */ jsx(DitherCanvas, { ramp: gemRamp(gem), order: "ramp", field, cell: cell ?? Math.max(2, Math.round(size / 12)), reveal, paintKey: gem })
    }
  );
}
export {
  DitherBand,
  DitherGem,
  DitherScene
};
