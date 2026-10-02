import { type ComponentProps, type ReactNode, type Ref } from "react";
export type HeadingLevel = 1 | 2 | 3 | 4;
/** What a caller may set on the heading element itself: `tabIndex={-1}` so code can focus it, an `id`, a `data-*`. */
export type HeadingProps = Omit<ComponentProps<"h1">, "children" | "className" | "ref">;
export declare function PageHeader({ title, description, actions, eyebrow, headingLevel, headingRef, headingProps, className, }: {
    title: ReactNode;
    description?: ReactNode;
    actions?: ReactNode;
    /** Context above the title, e.g. a Breadcrumb. Sentence case, not a tracked label. */
    eyebrow?: ReactNode;
    /** The heading element (default 1). The look is the page title's at every level. */
    headingLevel?: HeadingLevel;
    /** The heading element, for code that moves focus to it (give it `headingProps={{ tabIndex: -1 }}`). */
    headingRef?: Ref<HTMLHeadingElement>;
    headingProps?: HeadingProps;
    className?: string;
}): import("react").JSX.Element;
export declare function SectionHeader({ title, description, actions, aside, headingLevel, headingRef, headingProps, className, }: {
    title: ReactNode;
    description?: ReactNode;
    actions?: ReactNode;
    /**
     * A tag beside the title (a count, a status: "Connected mailboxes" `3 of 10`), outside the heading element so the
     * heading's accessible name stays the title. It sits on the title's line and wraps below it when the header is
     * narrow. Nothing changes without it (`false`, `null` and `""` mean no aside; a count of `0` is a tag).
     */
    aside?: ReactNode;
    /** The heading element (default 2). The look is the section title's at every level. */
    headingLevel?: HeadingLevel;
    /** The heading element, for code that moves focus to it (give it `headingProps={{ tabIndex: -1 }}`). */
    headingRef?: Ref<HTMLHeadingElement>;
    headingProps?: HeadingProps;
    className?: string;
}): import("react").JSX.Element;
export declare function CollectionToolbar({ search, filters, actions, label, }: {
    search?: ReactNode;
    filters?: ReactNode;
    actions?: ReactNode;
    label?: string;
}): import("react").JSX.Element;
export declare function BulkActions({ count, children, label, regionLabel, keepMounted, }: {
    count: number;
    children: ReactNode;
    label?: ReactNode;
    regionLabel?: string;
    /**
     * Keep the region in the DOM at a count of 0: the actions are `hidden` (they stay mounted, so a controller or a focus
     * handoff that relies on them keeps working) and the `role="status"` span is always there, visually hidden, so that
     * the change from 0 to 1 is announced. Without it the component renders nothing at 0.
     */
    keepMounted?: boolean;
}): import("react").JSX.Element | null;
/**
 * The parts of `BulkActions` for a layout the bar does not fit: the status in a title row (beside select-all and the folder
 * name), the actions in a form under it. `BulkActionsRoot` shares ONE count, ONE empty state and ONE `keepMounted` with
 * `BulkActionsStatus` and `BulkActionsContent`, and renders a plain `div` (no role, no bar look) that the consumer lays out
 * with `className` and puts the parts anywhere inside. `BulkActions` itself is unchanged.
 *
 * Labelling: the group is the actions (`BulkActionsContent`, `role="group"`, named by `regionLabel`); the status is a live
 * region beside it, not inside a group, so a title row that also holds select-all and a heading is not "Selection actions".
 *
 * At a count of 0 the parts render nothing unless `keepMounted`: then the status is always in the DOM (visually hidden at
 * 0, so the change to 1 is announced) and the actions are `hidden` but mounted (a controller or a focus handoff that relies
 * on them keeps working). The rest of what is inside the root (select-all, the heading) is never affected.
 */
export declare function BulkActionsRoot({ count, regionLabel, keepMounted, className, children, ...props }: Omit<ComponentProps<"div">, "role"> & {
    count: number;
    /** The accessible name of the actions group. */
    regionLabel?: string;
    keepMounted?: boolean;
}): import("react").JSX.Element;
/** The live status: `role="status"`, "N selected" unless it has children. Visually hidden (and still announced) at a count of 0 with `keepMounted`. */
export declare function BulkActionsStatus({ className, children, ...props }: ComponentProps<"span">): import("react").JSX.Element | null;
/**
 * The actions, a `role="group"` named by the root's `regionLabel` (`aria-label` overrides it). `hidden` is combined with the
 * empty state: a layout that keeps the actions closed until asked for (a phone) passes it, and they stay mounted. It takes a `ref`.
 */
export declare function BulkActionsContent({ className, children, hidden, ...props }: ComponentProps<"div">): import("react").JSX.Element | null;
declare const stateIcons: {
    loading: import("react").ForwardRefExoticComponent<Omit<import("lucide-react").LucideProps, "ref"> & import("react").RefAttributes<SVGSVGElement>>;
    empty: import("react").ForwardRefExoticComponent<Omit<import("lucide-react").LucideProps, "ref"> & import("react").RefAttributes<SVGSVGElement>>;
    error: import("react").ForwardRefExoticComponent<Omit<import("lucide-react").LucideProps, "ref"> & import("react").RefAttributes<SVGSVGElement>>;
    stale: import("react").ForwardRefExoticComponent<Omit<import("lucide-react").LucideProps, "ref"> & import("react").RefAttributes<SVGSVGElement>>;
    offline: import("react").ForwardRefExoticComponent<Omit<import("lucide-react").LucideProps, "ref"> & import("react").RefAttributes<SVGSVGElement>>;
    success: import("react").ForwardRefExoticComponent<Omit<import("lucide-react").LucideProps, "ref"> & import("react").RefAttributes<SVGSVGElement>>;
};
export type StatePanelProps = Omit<ComponentProps<"div">, "title" | "children"> & {
    state: keyof typeof stateIcons;
    title: ReactNode;
    description?: ReactNode;
    actions?: ReactNode;
    headingLevel?: 2 | 3 | 4;
    icon?: ReactNode;
    align?: "start" | "center";
    /** `sm` is 16 px of padding, for a drawer, a popover, a sidebar or a list slot; the icon and text stay the same. */
    size?: "md" | "sm";
    /** `inline` has no border and no tint (an overlay, a palette, a region that already sits in a panel); the icon still carries the state. */
    variant?: "panel" | "inline";
    /** Fill the height of the region and centre the content in it (an empty reader pane). */
    fill?: boolean;
};
export declare function StatePanel({ state, title, description, actions, headingLevel, className, icon, align, size, variant, fill, ...props }: StatePanelProps): import("react").JSX.Element;
export declare function Field({ label, description, error, children, id: providedId, }: {
    label: ReactNode;
    description?: ReactNode;
    error?: ReactNode;
    id?: string;
    children: (props: {
        id: string;
        "aria-describedby"?: string;
        "aria-invalid"?: true;
    }) => ReactNode;
}): import("react").JSX.Element;
export declare function WorkspaceShell({ navigation, header, children, contentId, skipLabel, className, ...props }: ComponentProps<"div"> & {
    navigation?: ReactNode;
    header?: ReactNode;
    contentId?: string;
    skipLabel?: string;
}): import("react").JSX.Element;
export declare function SiteHeader({ brand, navigation, actions, mobileMenu, sticky, className, ...props }: Omit<ComponentProps<"header">, "children"> & {
    brand: ReactNode;
    navigation?: ReactNode;
    actions?: ReactNode;
    /** Shown instead of the navigation below 768px, e.g. a Sheet trigger. */
    mobileMenu?: ReactNode;
    sticky?: boolean;
}): import("react").JSX.Element;
export declare function AuthLayout({ brand, title, description, children, footer, aside, actions, align, headingLevel, className, }: {
    brand?: ReactNode;
    title: ReactNode;
    description?: ReactNode;
    children: ReactNode;
    /** A short note about what the session can and cannot do. */
    footer?: ReactNode;
    /** Optional editorial panel shown beside the card on wide screens. */
    aside?: ReactNode;
    /**
     * A corner control for the page (an appearance and language menu). It sits at the inline end of the top edge, above the aside,
     * and comes AFTER the card in the DOM, so the first tab stop is the sign-in action, not the menu.
     */
    actions?: ReactNode;
    /**
     * Where the card sits in its column. `auto` (default) follows DESIGN.md: with an `aside` the card anchors to the inline start
     * of its column, next to the storm (from 1024 px; below it the aside is gone and the card centres), without one it centres.
     * `start` anchors it at every width, `center` centres it even beside an aside.
     */
    align?: "auto" | "start" | "center";
    headingLevel?: 1 | 2;
    className?: string;
}): import("react").JSX.Element;
export {};
