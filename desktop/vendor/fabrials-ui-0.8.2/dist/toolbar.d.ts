import { type ReactNode } from "react";
import { Toolbar as BaseToolbar } from "@base-ui/react/toolbar";
import type { IconButtonSize } from "./icon-button";
import { type ControlShortcut } from "./icon-tooltip";
import { type StyledProps } from "./shared";
import type { ButtonProps } from "./controls";
type Named = {
    "aria-label": string;
    "aria-labelledby"?: never;
} | {
    "aria-labelledby": string;
    "aria-label"?: never;
};
export type ToolbarProps = StyledProps<BaseToolbar.Root.Props> & Named & {
    /** `plain` is a bare row. `bar` is a band: the page background between two hairlines, its icons aligned to the
     * page gutter, and it measures its own width so `reveal` and `tier` respond to it. */
    variant?: "plain" | "bar";
    /** With `bar`: stays under the top of its scroll container (not on short windows, where it would take the screen). */
    sticky?: boolean;
    /** Whether the toolbar is the size container that `reveal` and `tier` read. On for `bar`. A container takes
     * its width from its parent, never from its buttons, so give a content-sized plain toolbar `false` and let a
     * pane above it be the container instead. */
    contain?: boolean;
};
/**
 * One tab stop for a row of controls. Tab enters at the last focused item (the first at the start) and leaves
 * on the next Tab; the arrow keys, Home and End move between the items (wrapping, unless `loopFocus={false}`),
 * so eight buttons cost one Tab, not eight. The name is required: a toolbar is a landmark-like group that a
 * screen reader announces by it.
 *
 * A toolbar is for buttons, toggles, menus and links. A native text field or select inside it keeps its own
 * arrow keys and adds a tab stop of its own; put those next to the toolbar instead.
 */
export declare function Toolbar({ className, variant, sticky, contain, onKeyDown, ...props }: ToolbarProps): import("react").JSX.Element;
export type ToolbarGroupProps = StyledProps<BaseToolbar.Group.Props> & Named;
/** Related items of a `Toolbar`, named for assistive technology (`aria-label` is required). `disabled` disables the group's items. */
export declare function ToolbarGroup({ className, ...props }: ToolbarGroupProps): import("react").JSX.Element;
/** A hairline between groups. It separates vertically in a horizontal toolbar; `tier="low"` hides it with the low-tier items. */
export declare function ToolbarSeparator({ className, tier, ...props }: StyledProps<BaseToolbar.Separator.Props> & {
    tier?: "high" | "low";
}): import("react").JSX.Element;
export type ToolbarButtonProps = Omit<StyledProps<BaseToolbar.Button.Props>, "aria-label" | "aria-labelledby" | "title" | "children"> & Pick<ButtonProps, "variant"> & {
    /** The name, as a text node inside the button (so `textContent`, voice control and find-in-page see it). It is
     * visually hidden until `reveal` shows it. Required: an icon is not a name. */
    label: string;
    /** The icon (16 px, `aria-hidden`). */
    children?: ReactNode;
    /** A tooltip repeats the name for sighted people (default); `false` leaves it out; a node replaces its text. */
    tooltip?: boolean | ReactNode;
    /** Keys shown in the tooltip and exposed as `aria-keyshortcuts`. */
    shortcut?: ControlShortcut;
    size?: IconButtonSize;
    /**
     * Shows the label beside the icon once the toolbar (or the container around it) is wide enough:
     * `early` from 40rem, `middle` from 52rem, `late` from 76rem. Below that it is an icon with a tooltip.
     */
    reveal?: "early" | "middle" | "late";
    /**
     * `high` (default) is always there. `low` is hidden below 34rem and belongs in an overflow menu then;
     * `overflow` is the trigger of that menu and is only there below 34rem. Hidden items leave the arrow-key
     * order, so the toolbar never loses its tab stop to an item nobody can see.
     */
    tier?: "high" | "low" | "overflow";
};
/**
 * An icon button that belongs to the toolbar's arrow-key order, named by a text node, with a tooltip.
 * A disabled button stays in the order (`aria-disabled`, no click) so its neighbours do not shift under a
 * person who is arrowing; pass `focusableWhenDisabled={false}` to take it out. Spread props let a Base UI trigger
 * render it: `<DropdownMenuTrigger render={<ToolbarButton label="More" tier="overflow"><Ellipsis /></ToolbarButton>} />`.
 * The tooltip wrapper is always mounted, so a control that becomes disabled during a refresh is never remounted.
 */
export declare function ToolbarButton({ label, children, tooltip, shortcut, size, variant, reveal, tier, className, disabled, focusableWhenDisabled, ref, ...props }: ToolbarButtonProps): import("react").JSX.Element;
export {};
