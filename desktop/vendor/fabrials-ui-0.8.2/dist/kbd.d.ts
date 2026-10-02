import { type ComponentProps, type ReactNode } from "react";
export declare function Kbd({ className, mod, children, ...props }: ComponentProps<"kbd"> & {
    /** Print the main modifier of this device ("Ctrl", or "⌘" on Apple after hydration) instead of `children`. */
    mod?: boolean;
}): import("react").JSX.Element;
/**
 * Keys pressed together, e.g. Ctrl + K. With `sequence` the keys are pressed one after the other: the group puts the
 * `separator` word between them, muted ("G then I"). The word is a prop because it is copy a product translates.
 */
export declare function KbdGroup({ className, sequence, separator, children, ...props }: ComponentProps<"kbd"> & {
    sequence?: boolean;
    separator?: ReactNode;
}): import("react").JSX.Element;
