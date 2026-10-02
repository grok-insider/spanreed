const MOON_CX = 256;
const MOON_CY = 256;
const MOON_R = 172;
const SYNODIC_DAYS = 29.530588853;
const KNOWN_NEW_MOON_MS = Date.UTC(2e3, 0, 6, 18, 14);
const BASE_GLOW = "#BFD3FF";
const SHADOW_OPACITY = 0.93;
const PHASE_NAMES = [
  "New moon",
  "Waxing crescent",
  "First quarter",
  "Waxing gibbous",
  "Full moon",
  "Waning gibbous",
  "Last quarter",
  "Waning crescent"
];
const MOON_VARIANTS = [
  { id: "regular", label: "Moon", rarity: "common", weight: 52, appearsAt: "any", scale: 1, glow: BASE_GLOW, glowIntensity: 1 },
  { id: "earthshine", label: "Earthshine", rarity: "uncommon", weight: 12, appearsAt: "crescent", scale: 1, glow: "#A9C4FF", glowIntensity: 1, earthshine: 0.34 },
  { id: "ice-halo", label: "Moon halo", rarity: "uncommon", weight: 10, appearsAt: "any", scale: 1, glow: "#CFE0FF", glowIntensity: 1.1, ring: 0.55 },
  { id: "harvest", label: "Harvest moon", rarity: "uncommon", weight: 7, appearsAt: "full", scale: 1.04, glow: "#FFC76B", glowIntensity: 1.15, tint: "#F2B35A", tintOpacity: 0.55 },
  { id: "supermoon", label: "Supermoon", rarity: "uncommon", weight: 7, appearsAt: "full", scale: 1.14, glow: "#E4EEFF", glowIntensity: 1.45 },
  { id: "micromoon", label: "Micromoon", rarity: "rare", weight: 5, appearsAt: "full", scale: 0.86, glow: BASE_GLOW, glowIntensity: 0.6 },
  { id: "blue", label: "Blue moon", rarity: "rare", weight: 4, appearsAt: "full", scale: 1, glow: "#8FB4FF", glowIntensity: 1.25, tint: "#9DBBFF", tintOpacity: 0.5 },
  { id: "blood", label: "Blood moon", rarity: "rare", weight: 2.5, appearsAt: "full", scale: 1, glow: "#FF5A3C", glowIntensity: 0.75, tint: "#B8321E", tintOpacity: 0.82 },
  { id: "super-blood", label: "Super blood moon", rarity: "legendary", weight: 0.5, appearsAt: "full", scale: 1.14, glow: "#FF4A2E", glowIntensity: 1, tint: "#A82A18", tintOpacity: 0.85 }
];
const RARITY_LABELS = {
  common: "Common",
  uncommon: "Uncommon",
  rare: "Rare",
  legendary: "Legendary"
};
function wrap(phase) {
  return (phase % 1 + 1) % 1;
}
function smoothstep(edge0, edge1, x) {
  const t = Math.min(1, Math.max(0, (x - edge0) / (edge1 - edge0)));
  return t * t * (3 - 2 * t);
}
function mixHex(from, to, t) {
  const a = Number.parseInt(from.slice(1), 16);
  const b = Number.parseInt(to.slice(1), 16);
  const channel = (shift) => Math.round((a >> shift & 255) + ((b >> shift & 255) - (a >> shift & 255)) * t);
  return `#${(channel(16) << 16 | channel(8) << 8 | channel(0)).toString(16).padStart(6, "0")}`;
}
function lunarPhase(date = /* @__PURE__ */ new Date()) {
  return wrap((date.getTime() - KNOWN_NEW_MOON_MS) / 864e5 / SYNODIC_DAYS);
}
function lunarPhaseName(phase) {
  return PHASE_NAMES[Math.round(wrap(phase) * 8) % 8];
}
function moonIllumination(phase) {
  return (1 - Math.cos(2 * Math.PI * wrap(phase))) / 2;
}
function terminator(phase) {
  const p = wrap(phase);
  const waxing = p < 0.5;
  const bulgesRight = waxing ? p < 0.25 : p < 0.75;
  return {
    rx: (Math.abs(Math.cos(2 * Math.PI * p)) * MOON_R).toFixed(2),
    shadowLimb: waxing ? 0 : 1,
    sweep: bulgesRight ? 0 : 1
  };
}
function moonShadowPath(phase) {
  const t = terminator(phase);
  return `M${MOON_CX},${MOON_CY - MOON_R} A${MOON_R} ${MOON_R} 0 0 ${t.shadowLimb} ${MOON_CX},${MOON_CY + MOON_R} A${t.rx} ${MOON_R} 0 0 ${t.sweep} ${MOON_CX},${MOON_CY - MOON_R} Z`;
}
function moonLitPath(phase) {
  const t = terminator(phase);
  return `M${MOON_CX},${MOON_CY - MOON_R} A${MOON_R} ${MOON_R} 0 0 ${1 - t.shadowLimb} ${MOON_CX},${MOON_CY + MOON_R} A${t.rx} ${MOON_R} 0 0 ${t.sweep} ${MOON_CX},${MOON_CY - MOON_R} Z`;
}
function isMoonVariantId(value) {
  return MOON_VARIANTS.some((variant) => variant.id === value);
}
function moonVariant(id) {
  return MOON_VARIANTS.find((variant) => variant.id === id) ?? MOON_VARIANTS[0];
}
function moonVariantCaption(variant) {
  return variant.id === "regular" ? "" : `${variant.label} · ${RARITY_LABELS[variant.rarity]}`;
}
function pickMoonVariant(random = Math.random) {
  const total = MOON_VARIANTS.reduce((sum, variant) => sum + variant.weight, 0);
  let roll = random() * total;
  for (const variant of MOON_VARIANTS) {
    roll -= variant.weight;
    if (roll < 0) return variant;
  }
  return MOON_VARIANTS[0];
}
function moonVariantStrength(variant, phase) {
  const lit = moonIllumination(phase);
  if (variant.appearsAt === "full") return smoothstep(0.82, 0.985, lit);
  if (variant.appearsAt === "crescent") return smoothstep(4e-3, 0.05, lit) * (1 - smoothstep(0.2, 0.45, lit));
  return 1;
}
function moonFrame(phase, variant = MOON_VARIANTS[0], reveal = 1) {
  const lit = moonIllumination(phase);
  const strength = moonVariantStrength(variant, phase) * reveal;
  const scale = 1 + (variant.scale - 1) * strength;
  const intensity = 1 + (variant.glowIntensity - 1) * strength;
  return {
    shadowPath: moonShadowPath(phase),
    litPath: moonLitPath(phase),
    transform: `translate(${MOON_CX} ${MOON_CY}) scale(${scale.toFixed(4)}) translate(${-MOON_CX} ${-MOON_CY})`,
    shadowOpacity: SHADOW_OPACITY - (variant.earthshine ?? 0) * strength,
    glowColor: mixHex(BASE_GLOW, variant.glow, strength),
    glowOuterOpacity: Math.min(1, intensity * (0.55 + 0.45 * lit)),
    glowInnerOpacity: Math.min(1, intensity * 0.85),
    tint: variant.tint ?? BASE_GLOW,
    tintOpacity: (variant.tintOpacity ?? 0) * strength,
    ringOpacity: (variant.ring ?? 0) * strength * (0.3 + 0.7 * lit),
    captionOpacity: variant.id === "regular" ? 0 : strength
  };
}
export {
  MOON_CX,
  MOON_CY,
  MOON_R,
  MOON_VARIANTS,
  isMoonVariantId,
  lunarPhase,
  lunarPhaseName,
  moonFrame,
  moonIllumination,
  moonLitPath,
  moonShadowPath,
  moonVariant,
  moonVariantCaption,
  moonVariantStrength,
  pickMoonVariant
};
