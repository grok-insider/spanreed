import { type ComponentProps, type ReactNode } from "react";
/** True inside a `Loading` that is showing its skeleton; lets a component trim work it cannot show. */
export declare function useLoading(): boolean;
export type LoadingProps = Omit<ComponentProps<"div">, "children"> & {
    /** Shows the skeleton. The children stay mounted, so nothing moves when it turns off. */
    when: boolean;
    /** What assistive technology hears while it loads, e.g. "Loading accounts". */
    label?: string;
    children: ReactNode;
};
/**
 * Turns whatever it wraps into its own skeleton while `when` is true: text
 * becomes a bar per line, icons and images become blocks, coloured controls
 * become neutral shapes, and borders, tables and cards stay as structure.
 * Render the real components with placeholder data (see `placeholderText`)
 * so the skeleton has the exact layout of the finished view.
 *
 * Layout-neutral: the wrappers use `display: contents`, only paint changes,
 * so the page does not shift when loading ends. The content is `inert` while
 * loading (no focus, hidden from assistive technology) and a status message
 * says what is loading. Works on the server; motion follows reduced motion.
 */
export declare function Loading({ when, label, className, children, ...props }: LoadingProps): import("react").JSX.Element;
/**
 * Placeholder copy of about `length` characters for a skeleton. Invisible in
 * the skeleton; only its length and word breaks matter, so lines wrap like the
 * real text will. Pass a `seed` for variety between rows.
 */
export declare function placeholderText(length: number, seed?: number): string;
/** `count` placeholder records built by `make`, for lists and tables while their data loads. */
export declare function placeholderList<T>(count: number, make: (index: number) => T): T[];
