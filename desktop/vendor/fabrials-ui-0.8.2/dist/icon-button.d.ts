import type { ReactNode } from "react";
import { type ButtonProps } from "./controls";
import { type ControlShortcut } from "./icon-tooltip";
export type { ControlShortcut } from "./icon-tooltip";
export type IconButtonSize = "icon" | "icon-xs" | "icon-sm" | "icon-lg";
export type IconButtonProps = Omit<ButtonProps, "aria-label" | "title" | "size" | "variant"> & {
    /** The control's name: `aria-label`, or a visually hidden text node with `textName`. Required: an icon is not a name. */
    label: string;
    /** A tooltip repeats the name for sighted people (default), `false` leaves it out, a node replaces its text. */
    tooltip?: boolean | ReactNode;
    /** Keys shown in the tooltip as flat `Kbd`s, and exposed as `aria-keyshortcuts` ("⌘", "K"). Keys pressed one after the other are `{ keys: ["g", "i"], sequence: true }`: the tooltip says "g then i" and no `aria-keyshortcuts` is set (it cannot express steps). */
    shortcut?: ControlShortcut;
    /** The name is a text node inside the button (for toolbars whose tests, voice control or find-in-page read
     * `textContent`) instead of `aria-label`. It is visually hidden. */
    textName?: boolean;
    size?: IconButtonSize;
    variant?: ButtonProps["variant"];
};
/**
 * An icon-only button that always has a name: 44 px, ghost, named by `label`, with a tooltip on hover and on focus.
 * Do not add `title` (it would announce the name twice). It spreads its props on the `Button`, so a Base UI trigger
 * can render it: `<DialogTrigger render={<IconButton label="Rename"><Pencil /></IconButton>} />`.
 * The tooltip wrapper is always mounted, so a control that becomes disabled keeps its identity.
 * `loading` swaps the icon for the spinner and blocks repeated presses.
 */
export declare function IconButton({ label, tooltip, shortcut, textName, size, variant, loading, children, ...props }: IconButtonProps): import("react").JSX.Element;
