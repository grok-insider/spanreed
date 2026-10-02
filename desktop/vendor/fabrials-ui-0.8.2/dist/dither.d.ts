/**
 * Ordered (Bayer 8×8) dithering for decorative backgrounds: brightness onto
 * a short colour ramp, or per-channel quantisation that keeps hues. Pure and
 * server-safe (no DOM), so hosts and tests can use it without React. The
 * canvas component is DitherCanvas.
 */
export type Rgb = [number, number, number];
/** 8×8 Bayer thresholds, normalised to (0, 1). */
export declare const BAYER_8: Float32Array<ArrayBuffer>;
export declare function hexToRgb(hex: string): Rgb;
export declare function rgbToHex([r, g, b]: Rgb): string;
/** Linear mix of two #rrggbb colours, `t` of the way from `a` to `b`. */
export declare function mixHex(a: string, b: string, t: number): string;
/** Integer brightness 0–255, weighted toward green like the eye. */
export declare function luma([r, g, b]: Rgb): number;
export declare function sortByLuma(colors: Rgb[]): Rgb[];
/**
 * Rewrites RGBA pixels in place: each pixel's brightness picks a spot on the
 * ramp (sorted dark → light) and the Bayer threshold decides between the two
 * neighbouring ramp colours. Output is opaque and uses ramp colours only.
 */
export declare function ditherToRamp(data: Uint8ClampedArray, width: number, ramp: Rgb[]): void;
/**
 * Per-channel ordered dithering for images that keep their hues: each
 * channel is mapped into `lo…hi` (0–255), then quantised to `levels` evenly
 * spaced values, with the Bayer threshold choosing between the two nearest.
 */
export declare function ditherChannels(data: Uint8ClampedArray, width: number, levels: number, lo: number, hi: number): void;
/** Small deterministic PRNG (mulberry32): the same seed paints the same picture. */
export declare function seeded(seed: number): () => number;
/** A stable 32-bit seed from any string (an id, a handle). */
export declare function seedFrom(text: string): number;
export type BrightnessField = (u: number, v: number) => number;
/**
 * Renders a brightness field (0–1 at unit coordinates) into RGBA pixels on
 * the ramp's luma range, dithered onto the ramp. Pure: DitherCanvas calls it
 * with ImageData, tests with a plain array.
 */
export declare function paintField(data: Uint8ClampedArray, width: number, height: number, field: BrightnessField, ramp: Rgb[]): void;
/**
 * Like `paintField`, but the ramp keeps its order: field 0 is `ramp[0]` (the
 * surface behind the canvas) and 1 is the last colour (the brightest light).
 * Brand scenes use it because in the light theme the "light" is darker than
 * the background, which a brightness-sorted ramp cannot express.
 */
export declare function paintRampField(data: Uint8ClampedArray, width: number, height: number, field: BrightnessField, ramp: Rgb[]): void;
export * from "./dither-presets";
