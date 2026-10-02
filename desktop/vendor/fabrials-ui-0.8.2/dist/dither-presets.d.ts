/**
 * The Highstorm brand fields and ramps (0.7): a dithered storm front for
 * landings and sign-in, a cloud band for section headers, and a cut gem for
 * product marks. Pure; DitherScene, DitherBand and DitherGem render them.
 * Ramps run from the surface behind the canvas ("background", resolved by
 * DitherCanvas) to the brightest light, for `order="ramp"`.
 */
import type { BrightnessField } from "./dither";
/** Resolved by DitherCanvas to the surface behind it (`--fui-dither-bg`, else `--background`). */
export declare const DITHER_BACKGROUND = "background";
export type ThemeRamp = {
    light: string[];
    dark: string[];
};
/** Slate and crem clouds with Stormlight at the core. */
export declare const STORM_RAMP: ThemeRamp;
/** Gem colours as hex, for ramps (the CSS tokens are oklch). */
export declare const GEM_HEX: {
    readonly stormlight: "#3f8fd9";
    readonly heliodor: "#d6a13b";
    readonly sapphire: "#3b5bc4";
    readonly ruby: "#c73b45";
    readonly emerald: "#23896a";
    readonly zircon: "#36a3b5";
    readonly smokestone: "#76705f";
    readonly amethyst: "#8b55c6";
};
export type GemHexName = keyof typeof GEM_HEX;
/** Background → gem → highlight. The first stop is resolved from the surface. */
export declare function gemRamp(gem: GemHexName): ThemeRamp;
/** Smooth value noise, 0–1. */
export declare function valueNoise(x: number, y: number, seed?: number): number;
/** Five octaves of value noise, 0–1. */
export declare function fbm(x: number, y: number, seed?: number): number;
export type StormOptions = {
    /** Where the front starts and is fully in, across the width (0–1). */
    front?: [number, number];
    seed?: number;
    /** Fade the top and bottom edges so the scene sits in a band without hard cuts. */
    fadeEdges?: boolean;
};
/**
 * A storm front rolling in from the right. `progress` (0–1) slides the front
 * in for a one-time reveal; 1 is the resting picture. `aspect` is width/height.
 */
export declare function stormField(aspect: number, progress?: number, { front, seed, fadeEdges }?: StormOptions): BrightnessField;
/** A low cloud band that brightens to the right, for section headers. */
export declare function stormBandField(aspect: number, seed?: number): BrightnessField;
/**
 * An octagonal cut gem lit from the top left, square in any box: table,
 * eight facets and a girdle. `progress` fills it with light from the bottom.
 */
export declare function gemField(width: number, height: number, progress?: number, size?: number): BrightnessField;
