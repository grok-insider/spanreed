import { DITHER_BACKGROUND, GEM_HEX, STORM_RAMP, fbm, gemField, gemRamp, stormBandField, stormField, valueNoise } from "./dither-presets.js";
const BAYER_8 = (() => {
  const m = [
    [0, 32, 8, 40, 2, 34, 10, 42],
    [48, 16, 56, 24, 50, 18, 58, 26],
    [12, 44, 4, 36, 14, 46, 6, 38],
    [60, 28, 52, 20, 62, 30, 54, 22],
    [3, 35, 11, 43, 1, 33, 9, 41],
    [51, 19, 59, 27, 49, 17, 57, 25],
    [15, 47, 7, 39, 13, 45, 5, 37],
    [63, 31, 55, 23, 61, 29, 53, 21]
  ];
  return Float32Array.from(m.flat(), (v) => (v + 0.5) / 64);
})();
const BAYER_RANK = Uint8Array.from(BAYER_8, (v) => Math.floor(v * 64));
function hexToRgb(hex) {
  const n = Number.parseInt(hex.replace("#", ""), 16);
  return [n >> 16 & 255, n >> 8 & 255, n & 255];
}
function rgbToHex([r, g, b]) {
  return `#${[r, g, b].map((v) => Math.round(v).toString(16).padStart(2, "0")).join("")}`;
}
function mixHex(a, b, t) {
  const x = hexToRgb(a);
  const y = hexToRgb(b);
  return rgbToHex(x.map((v, i) => v + (y[i] - v) * t));
}
function luma([r, g, b]) {
  return r * 2 + g * 5 + b >> 3;
}
function sortByLuma(colors) {
  return [...colors].sort((a, b) => luma(a) - luma(b));
}
function ditherToRamp(data, width, ramp) {
  if (ramp.length < 2) throw new Error("ditherToRamp needs at least two colours");
  const levels = ramp.map(luma);
  const last = ramp.length - 1;
  const lower = new Uint8Array(256);
  const toward = new Uint8Array(256);
  for (let v = 0, step = 0; v < 256; v++) {
    while (step < last - 1 && v >= levels[step + 1]) step++;
    const span = levels[step + 1] - levels[step];
    lower[v] = step;
    toward[v] = span > 0 ? Math.max(0, Math.min(64, Math.round((v - levels[step]) / span * 64))) : 0;
  }
  const packed = new Uint32Array(ramp.length);
  new Uint8Array(packed.buffer).set(ramp.flatMap(([r, g, b]) => [r, g, b, 255]));
  const pixels = new Uint32Array(data.buffer, data.byteOffset, data.length >> 2);
  const height = pixels.length / width;
  for (let y = 0, p = 0, i = 0; y < height; y++) {
    const row = (y & 7) << 3;
    for (let x = 0; x < width; x++, p++, i += 4) {
      const v = data[i] * 2 + data[i + 1] * 5 + data[i + 2] >> 3;
      pixels[p] = packed[lower[v] + (toward[v] > BAYER_RANK[row | x & 7] ? 1 : 0)];
    }
  }
}
function ditherChannels(data, width, levels, lo, hi) {
  const steps = Math.max(2, Math.round(levels)) - 1;
  const span = hi - lo;
  const lower = new Uint8Array(256);
  const toward = new Uint8Array(256);
  for (let v = 0; v < 256; v++) {
    const t = v / 255 * steps;
    const base = Math.min(steps - 1, Math.floor(t));
    lower[v] = base;
    toward[v] = Math.round((t - base) * 64);
  }
  const value = Uint8Array.from({ length: steps + 1 }, (_, k) => Math.round(lo + span * k / steps));
  const height = data.length / 4 / width;
  for (let y = 0, i = 0; y < height; y++) {
    const row = (y & 7) << 3;
    for (let x = 0; x < width; x++, i += 4) {
      const rank = BAYER_RANK[row | x & 7];
      for (let c = 0; c < 3; c++) {
        const v = data[i + c];
        data[i + c] = value[lower[v] + (toward[v] > rank ? 1 : 0)];
      }
      data[i + 3] = 255;
    }
  }
}
function seeded(seed) {
  let a = seed >>> 0;
  return () => {
    a = a + 1831565813 >>> 0;
    let t = a;
    t = Math.imul(t ^ t >>> 15, t | 1);
    t ^= t + Math.imul(t ^ t >>> 7, t | 61);
    return ((t ^ t >>> 14) >>> 0) / 4294967296;
  };
}
function seedFrom(text) {
  let h = 2166136261;
  for (let i = 0; i < text.length; i++) {
    h ^= text.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return h >>> 0;
}
function paintField(data, width, height, field, ramp) {
  const sorted = sortByLuma(ramp);
  const low = luma(sorted[0]);
  const span = luma(sorted[sorted.length - 1]) - low;
  for (let y = 0, i = 0; y < height; y++) {
    const v = height > 1 ? y / (height - 1) : 0;
    for (let x = 0; x < width; x++, i += 4) {
      const b = field(width > 1 ? x / (width - 1) : 0, v);
      const g = Math.round(low + span * Math.min(1, Math.max(0, Number.isFinite(b) ? b : 0)));
      data[i] = data[i + 1] = data[i + 2] = g;
      data[i + 3] = 255;
    }
  }
  ditherToRamp(data, width, sorted);
}
function paintRampField(data, width, height, field, ramp) {
  if (ramp.length < 2) throw new Error("paintRampField needs at least two colours");
  const last = ramp.length - 1;
  for (let y = 0, i = 0; y < height; y++) {
    const row = (y & 7) << 3;
    const v = height > 1 ? y / (height - 1) : 0;
    for (let x = 0; x < width; x++, i += 4) {
      const b = field(width > 1 ? x / (width - 1) : 0, v);
      const t = Math.min(1, Math.max(0, Number.isFinite(b) ? b : 0)) * last;
      const lo = Math.min(last - 1, Math.floor(t));
      const [r, g, bl] = ramp[t - lo > BAYER_8[row | x & 7] ? lo + 1 : lo];
      data[i] = r;
      data[i + 1] = g;
      data[i + 2] = bl;
      data[i + 3] = 255;
    }
  }
}
export {
  BAYER_8,
  DITHER_BACKGROUND,
  GEM_HEX,
  STORM_RAMP,
  ditherChannels,
  ditherToRamp,
  fbm,
  gemField,
  gemRamp,
  hexToRgb,
  luma,
  mixHex,
  paintField,
  paintRampField,
  rgbToHex,
  seedFrom,
  seeded,
  sortByLuma,
  stormBandField,
  stormField,
  valueNoise
};
