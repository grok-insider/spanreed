import type { ComponentProps, ReactNode } from "react";
import { Popover } from "./popover";
/**
 * A status that is one sentence long, in a toolbar or a header: an icon (or an icon and a word) that opens an anchored popover
 * with the title, the description and what a person can do about it (`children`, usually a button). It is not a modal and not
 * a tooltip: Base UI gives the popup `role="dialog"` named by the title, and Escape returns focus to the trigger.
 *
 * `attention` tints the trigger with a status ink (`true` is danger; `"warning"`), and is never the only signal: the icon
 * should change with the state and `label` adds a visible word. Without `label` the trigger is a 44 px icon button named
 * "title: description". With `label` the name is the title and the label; `compact` hides the label visually (a narrow bar) and keeps it
 * in the name. The trigger carries `title={description}` for hover.
 */
export declare function StatusPopover({ title, description, icon, label, attention, compact, side, align, keepMounted, className, children, ...root }: Omit<ComponentProps<typeof Popover>, "children"> & {
    title: string;
    description: string;
    icon: ReactNode;
    /** A visible word beside the icon. */
    label?: string;
    attention?: boolean | "warning" | "danger";
    /** Icon only, whatever the width; the label stays in the accessible name. */
    compact?: boolean;
    side?: "top" | "bottom" | "left" | "right";
    align?: "start" | "center" | "end";
    keepMounted?: boolean;
    className?: string;
    /** What to do about it, shown under the description. */
    children?: ReactNode;
}): import("react").JSX.Element;
