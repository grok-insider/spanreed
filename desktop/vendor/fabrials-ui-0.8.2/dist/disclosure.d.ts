import type { ComponentProps, ReactNode } from "react";
/**
 * A native `details` element. It opens and closes with Enter and Space on its summary without any script, is
 * server-rendered exactly as it hydrates, keeps its panel mounted (form values, a search and the browser's
 * find-in-page all see closed content) and takes `open`, `onToggle` and `name` (an exclusive group) as usual.
 *
 * When a field inside a closed disclosure fails validation the disclosure opens itself, so the browser can
 * focus the field and show its message instead of failing silently; turn it off with `revealInvalid={false}`.
 */
export declare function Disclosure({ className, revealInvalid, onInvalidCapture, ...props }: ComponentProps<"details"> & {
    revealInvalid?: boolean;
}): import("react").JSX.Element;
/**
 * The always-visible line of a `Disclosure`, a target of at least 44 px (`size="sm"` paints a quiet 13 px line
 * and extends the target invisibly). `count` sits after the label inside the summary, so it is part of the
 * control's name and a screen reader reads "Conversation 12". `chevron="end"` (default) turns from down to up
 * when open; `"start"` points to the inline end when closed and down when open; `"none"` leaves the chevron out.
 */
export declare function DisclosureSummary({ className, children, count, chevron, size, ...props }: ComponentProps<"summary"> & {
    count?: ReactNode;
    chevron?: "end" | "start" | "none";
    size?: "sm" | "md" | "lg";
}): import("react").JSX.Element;
/** The content of a `Disclosure`, with the house spacing under the summary. */
export declare function DisclosurePanel({ className, ...props }: ComponentProps<"div">): import("react").JSX.Element;
