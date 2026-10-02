import type { ComponentProps, ReactNode } from "react";
export type GemName = "stormlight" | "heliodor" | "sapphire" | "ruby" | "emerald" | "zircon" | "smokestone" | "amethyst";
/** The faceted gem at the heart of every Fabrials mark. Colored by `--gem`. */
export declare function FabrialsGem({ size, gem, className, title, ...props }: Omit<ComponentProps<"svg">, "children"> & {
    size?: number;
    gem?: GemName;
    /** Accessible name. Without it the gem is decorative. */
    title?: string;
}): import("react").JSX.Element;
export type ProductLockupProps = Omit<ComponentProps<"span">, "children"> & {
    product: ReactNode;
    tagline?: ReactNode;
    gem?: GemName;
    /** Replaces the gem, for example with a product illustration in marketing. */
    mark?: ReactNode;
    size?: "sm" | "md" | "lg";
};
export declare function ProductLockup({ product, tagline, gem, mark, size, className, ...props }: ProductLockupProps): import("react").JSX.Element;
