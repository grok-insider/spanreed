import type { ComponentProps } from "react";
import type { GemName } from "./brand";
export type DitherSceneProps = Omit<ComponentProps<"div">, "children"> & {
    /** `hero`: the front covers the right of the box. `full`: it covers nearly all of it (sign-in). */
    variant?: "hero" | "full";
    seed?: number;
    /** One-time reveal in ms when the scene first paints; 0 turns it off. */
    reveal?: number;
    /** CSS pixels per dither dot. */
    cell?: number;
};
/**
 * The Highstorm storm front behind a landing hero or a sign-in page. It fills
 * its box (position the box, the scene is absolute) and fades into the
 * surface behind it. Keep text off the dense part: put it on the calm side or
 * on a solid surface.
 */
export declare function DitherScene({ variant, seed, reveal, cell, className, ...props }: DitherSceneProps): import("react").JSX.Element;
export type DitherBandProps = Omit<ComponentProps<"div">, "children"> & {
    seed?: number;
    cell?: number;
};
/** A thin storm band that signs a section or page header. Decorative; put the heading below it. */
export declare function DitherBand({ seed, cell, className, ...props }: DitherBandProps): import("react").JSX.Element;
export type DitherGemProps = Omit<ComponentProps<"span">, "children"> & {
    /** The product's gem. */
    gem?: GemName;
    /** Box size in CSS pixels. */
    size?: number;
    /** CSS pixels per dither dot; defaults to one dot per ~10th of the size, at least 2. */
    cell?: number;
    /** One-time fill with light in ms (large marks only). */
    reveal?: number;
};
/**
 * A cut gem filled with dithered light: the product mark in navigation,
 * index rows, empty states and, large, beside a hero. Decorative; pair it
 * with the product name.
 */
export declare function DitherGem({ gem, size, cell, reveal, className, style, ...props }: DitherGemProps): import("react").JSX.Element;
