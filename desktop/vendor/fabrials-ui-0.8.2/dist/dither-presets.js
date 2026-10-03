const DITHER_BACKGROUND = "background";
const STORM_RAMP = {
  dark: [DITHER_BACKGROUND, "#1c2129", "#252b35", "#343b47", "#4a4d52", "#6f7d92", "#a9c3e8", "#dbe8ff"],
  light: [DITHER_BACKGROUND, "#dcdfe0", "#c5cad0", "#a3abb5", "#7b8593", "#56606e", "#3a4351", "#2a63c4"]
};
const GEM_HEX = {
  stormlight: "#3f8fd9",
  heliodor: "#d6a13b",
  sapphire: "#3b5bc4",
  ruby: "#c73b45",
  emerald: "#23896a",
  zircon: "#36a3b5",
  smokestone: "#76705f",
  amethyst: "#8b55c6"
};
function mix(a, b, t) {
  const p = (h) => {
    const n = Number.parseInt(h.slice(1), 16);
    return [n >> 16 & 255, n >> 8 & 255, n & 255];
  };
  const x = p(a);
  const y = p(b);
  return `#${x.map((v, i) => Math.round(v + (y[i] - v) * t).toString(16).padStart(2, "0")).join("")}`;
}
function gemRamp(gem) {
  const g = GEM_HEX[gem];
  return {
    dark: [DITHER_BACKGROUND, mix("#161a21", g, 0.3), mix("#161a21", g, 0.62), g, mix(g, "#ffffff", 0.35), mix(g, "#ffffff", 0.75)],
    light: [DITHER_BACKGROUND, mix("#eceeeb", g, 0.35), mix("#eceeeb", g, 0.7), g, mix(g, "#10131a", 0.35), mix(g, "#ffffff", 0.6)]
  };
}
function hash(x, y, s) {
  let h = Math.imul(x | 0, 374761393) ^ Math.imul(y | 0, 668265263) ^ Math.imul(s | 0, 1274126177);
  h = Math.imul(h ^ h >>> 13, 1274126177);
  h ^= h >>> 16;
  return (h >>> 0) / 4294967296;
}
function valueNoise(x, y, seed = 0) {
  const xi = Math.floor(x);
  const yi = Math.floor(y);
  const xf = x - xi;
  const yf = y - yi;
  const u = xf * xf * (3 - 2 * xf);
  const v = yf * yf * (3 - 2 * yf);
  const a = hash(xi, yi, seed);
  const b = hash(xi + 1, yi, seed);
  const c = hash(xi, yi + 1, seed);
  const d = hash(xi + 1, yi + 1, seed);
  return a + (b - a) * u + (c - a) * v + (a - b - c + d) * u * v;
}
function fbm(x, y, seed = 0) {
  let total = 0;
  let amplitude = 0.5;
  let frequency = 1;
  for (let i = 0; i < 5; i++) {
    total += amplitude * valueNoise(x * frequency, y * frequency, seed + i * 17);
    frequency *= 2.03;
    amplitude *= 0.5;
  }
  return total / 0.96875;
}
const smooth = (a, b, x) => {
  const t = Math.min(1, Math.max(0, (x - a) / (b - a)));
  return t * t * (3 - 2 * t);
};
function stormField(aspect, progress = 1, { front = [0.12, 0.62], seed = 7, fadeEdges = true } = {}) {
  const shift = (1 - progress) * 0.75;
  return (u, v) => {
    let f = smooth(front[0] + shift, front[1] + shift, u + (0.5 - v) * 0.3);
    if (fadeEdges) f *= smooth(0, 0.14, v) * smooth(1, 0.84, v);
    if (f <= 0) return 0;
    const billow = Math.pow(smooth(0.2, 0.85, fbm(u * 2.6 * Math.min(aspect, 2.4), v * 2.6, seed)), 1.25);
    const lit = 0.55 + 0.9 * (1 - v) * u;
    const shaft = Math.exp(-Math.pow((u - 0.72 - v * 0.14) / 0.04, 2)) * (1 - v) * 0.3;
    return Math.min(0.9, f * (billow * lit * 1.35 + shaft));
  };
}
function stormBandField(aspect, seed = 33) {
  return (u, v) => Math.pow(fbm(u * aspect * 0.9, v * 1.4, seed), 1.5) * (0.25 + 1.1 * u);
}
function gemField(width, height, progress = 1, size = 0.5) {
  const r0 = Math.min(width, height) * size;
  const level = 1 - progress * 1.15;
  const light = -2.2;
  return (u, v) => {
    const dx = (u - 0.5) * width / r0;
    const dy = (v - 0.5) * height / r0;
    const ax = Math.abs(dx);
    const ay = Math.abs(dy);
    const r = Math.max(ax, ay, (ax + ay) * 0.7071);
    if (r > 1) return 0;
    let value;
    if (r > 0.94) value = 0.5;
    else if (r < 0.56) value = 0.6 + 0.3 * (-(dx + dy) * 0.5 / 0.56);
    else {
      const sector = Math.round(Math.atan2(dy, dx) / (Math.PI / 4)) * (Math.PI / 4);
      value = (0.26 + 0.6 * (0.5 + 0.5 * Math.cos(sector - light))) * (0.88 + 0.12 * (r - 0.56) / 0.38);
    }
    if ((dy + 1) / 2 < level) value = Math.min(value, 0.16);
    return value;
  };
}
export {
  DITHER_BACKGROUND,
  GEM_HEX,
  STORM_RAMP,
  fbm,
  gemField,
  gemRamp,
  stormBandField,
  stormField,
  valueNoise
};
