import { type ComponentProps } from "react";
import { Tabs as BaseTabs } from "@base-ui/react/tabs";
import { type StyledProps } from "./shared";
export type Tone = "neutral" | "info" | "success" | "warning" | "danger";
export declare function Badge({ className, tone, variant, dot, dotColor, truncate, style, title, children, ...props }: ComponentProps<"span"> & {
    tone?: Tone | "accent";
    variant?: "soft" | "outline" | "solid";
    /** Adds a leading status dot; the text remains the accessible meaning. `"hollow"` is an outline dot (an archived or inactive item). */
    dot?: boolean | "hollow";
    /**
     * Colours the dot only, with any CSS colour (`var(--label-tone)`); it sets `--fui-badge-dot`. To colour the tag itself
     * (tint, hairline and dot) from data, set `--fui-badge-solid` on the badge itself (a class or `style`); `--fui-badge-ink`
     * is the text colour, set the same way. Both are public properties, and the tone attribute only sets their defaults.
     * (`--fui-badge-dot` is not declared on the badge, so it also works from an ancestor.)
     */
    dotColor?: string;
    /**
     * The tag shrinks to its container and cuts long text with an ellipsis (the full text goes to `title` when it is a
     * string and no title is given). `--fui-badge-max` caps the width (default: 100% of the container).
     */
    truncate?: boolean;
}): import("react").JSX.Element;
export declare function Card({ className, interactive, ...props }: ComponentProps<"section"> & {
    /** Hover and focus-within affordance for cards that contain one primary link. */
    interactive?: boolean;
}): import("react").JSX.Element;
export declare function CardHeader({ className, ...props }: ComponentProps<"div">): import("react").JSX.Element;
export declare function CardTitle({ className, as: Heading, ...props }: ComponentProps<"h2"> & {
    as?: "h2" | "h3" | "h4";
}): import("react").JSX.Element;
export declare function CardDescription({ className, ...props }: ComponentProps<"p">): import("react").JSX.Element;
export declare function CardAction({ className, ...props }: ComponentProps<"div">): import("react").JSX.Element;
export declare function CardContent({ className, ...props }: ComponentProps<"div">): import("react").JSX.Element;
export declare function CardFooter({ className, ...props }: ComponentProps<"div">): import("react").JSX.Element;
export declare function Alert({ className, variant, layout, children, ...props }: ComponentProps<"div"> & {
    variant?: "default" | "info" | "success" | "warning" | "destructive";
    /**
     * `inline` is a slim notice for the top of a page or a pane: from a width of 48rem the title and the description share
     * one line and the action sits at the end; narrower it stacks like the default. It measures its own width, so it fills
     * its container (give it `flex: 1` in a flex row). Not for status that refreshes periodically: a notice is a one-off.
     */
    layout?: "stacked" | "inline";
}): import("react").JSX.Element;
export declare function AlertTitle({ className, ...props }: ComponentProps<"div">): import("react").JSX.Element;
export declare function AlertDescription({ className, ...props }: ComponentProps<"div">): import("react").JSX.Element;
export declare function AlertAction({ className, ...props }: ComponentProps<"div">): import("react").JSX.Element;
export declare function Separator({ className, orientation, ...props }: ComponentProps<"hr"> & {
    orientation?: "horizontal" | "vertical";
}): import("react").JSX.Element;
export declare function Skeleton({ className, ...props }: ComponentProps<"div">): import("react").JSX.Element;
export declare function Progress({ className, tone, ...props }: ComponentProps<"progress"> & {
    tone?: Tone;
}): import("react").JSX.Element;
export declare function Table({ className, children, regionLabel, stickyHeader, ...props }: ComponentProps<"table"> & {
    regionLabel?: string;
    stickyHeader?: boolean;
}): import("react").JSX.Element;
/**
 * The tabs root. Horizontal tabs need no layout of their own; with `orientation="vertical"` the list sits beside
 * the panel (a rail: `TabsList` styles itself for it) and the panel takes the rest.
 */
export declare function Tabs({ className, ...props }: StyledProps<BaseTabs.Root.Props>): import("react").JSX.Element;
export declare function TableHeader(props: ComponentProps<"thead">): import("react").JSX.Element;
export declare function TableBody(props: ComponentProps<"tbody">): import("react").JSX.Element;
export declare function TableFooter(props: ComponentProps<"tfoot">): import("react").JSX.Element;
export declare function TableRow(props: ComponentProps<"tr">): import("react").JSX.Element;
export declare function TableHead({ numeric, ...props }: ComponentProps<"th"> & {
    numeric?: boolean;
}): import("react").JSX.Element;
export declare function TableCell({ numeric, ...props }: ComponentProps<"td"> & {
    numeric?: boolean;
}): import("react").JSX.Element;
export declare function TableCaption(props: ComponentProps<"caption">): import("react").JSX.Element;
export declare function TabsList({ className, variant, scrollable, ref, ...props }: StyledProps<BaseTabs.List.Props> & {
    variant?: "underline" | "segmented";
    /**
     * One line that scrolls sideways instead of wrapping: no scrollbar, snap to the tabs, and the selected tab is kept
     * in view. `true` always; `"narrow"` only below 48rem of viewport (wraps above it, for a few tabs on a wide screen).
     */
    scrollable?: boolean | "narrow";
}): import("react").JSX.Element;
export declare function TabsTrigger({ className, ...props }: StyledProps<BaseTabs.Tab.Props>): import("react").JSX.Element;
export declare function TabsContent({ className, ...props }: StyledProps<BaseTabs.Panel.Props>): import("react").JSX.Element;
