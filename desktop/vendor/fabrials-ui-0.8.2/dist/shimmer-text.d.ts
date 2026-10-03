import type { ComponentPropsWithoutRef, ElementType } from "react";
export type ShimmerTextProps = Omit<ComponentPropsWithoutRef<"span">, "children"> & {
    children: string;
    as?: ElementType;
    duration?: number;
    spread?: number;
};
export declare function ShimmerText({ children, as: Component, className, duration, spread, style, ...props }: ShimmerTextProps): import("react").JSX.Element;
