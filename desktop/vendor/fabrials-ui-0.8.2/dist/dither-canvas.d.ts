import { type CSSProperties } from "react";
import { type BrightnessField } from "./dither";
export type DitherTheme = "light" | "dark";
/**
 * Builds the brightness field for one paint; called again on resize and theme
 * change. `progress` runs 0 → 1 once when `reveal` is set, and is 1 otherwise.
 */
export type DitherFieldFactory = (theme: DitherTheme, size: {
    width: number;
    height: number;
}, progress: number) => BrightnessField;
export type DitherStars = {
    /** Stars per 1,000 dots. */
    density: number;
    colors?: string[];
    /** Fraction of the height stars may reach from the top (0–1). */
    reach?: number;
    seed?: number;
};
export type DitherCanvasProps = {
    /**
     * Colours, one ramp or one per theme. With `order="brightness"` (default)
     * any order, sorted dark → light. With `order="ramp"` the order is kept:
     * field 0 is the first colour, 1 the last. A stop named "background" is the
     * surface behind the canvas (`--fui-dither-bg`, else `--background`).
     */
    ramp: string[] | {
        light: string[];
        dark: string[];
    };
    order?: "brightness" | "ramp";
    /** Milliseconds for a one-time reveal (the field gets progress 0 → 1). Skipped under reduced motion. */
    reveal?: number;
    field: DitherFieldFactory;
    /** CSS pixels per dither dot. */
    cell?: number;
    /** Crisp single-dot stars on top, only where the theme is dark. */
    stars?: DitherStars;
    /** Pin to the viewport instead of the nearest positioned ancestor. */
    fixed?: boolean;
    /** Changing it repaints (use for data-driven fields). */
    paintKey?: string | number;
    className?: string;
    style?: CSSProperties;
};
/**
 * A decorative ordered-dither background painted on a canvas: the host
 * supplies a brightness field and a colour ramp per theme (the nearest
 * `.dark`/`data-theme` ancestor decides, then the document root). Painted
 * once, and again only when its size, the theme or `paintKey` change. There
 * is no animation loop. The parent must be positioned and isolated
 * (`position: relative; isolation: isolate`) unless `fixed` is set.
 */
export declare function DitherCanvas({ ramp, field, cell, order, reveal, stars, fixed, paintKey, className, style }: DitherCanvasProps): import("react").JSX.Element;
