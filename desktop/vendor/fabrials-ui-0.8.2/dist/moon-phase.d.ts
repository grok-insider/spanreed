import { type CSSProperties } from "react";
import { type MoonVariant, type MoonVariantId } from "./moon-math";
export type MoonPhaseProps = {
    phase?: number;
    animate?: boolean;
    cycleMs?: number;
    size?: number | string;
    halo?: boolean;
    variant?: MoonVariantId | "random";
    caption?: boolean;
    onVariantChange?: (variant: MoonVariant) => void;
    label?: string;
    className?: string;
    style?: CSSProperties;
};
export declare function MoonPhase({ phase, animate, cycleMs, size, halo, variant, caption, onVariantChange, label, className, style, }: MoonPhaseProps): import("react").JSX.Element;
export type StarfieldProps = {
    twinkle?: boolean;
    className?: string;
};
export declare function Starfield({ twinkle, className }: StarfieldProps): import("react").JSX.Element;
