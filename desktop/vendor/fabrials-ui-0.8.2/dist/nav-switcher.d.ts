import { type ComponentProps, type ReactNode } from "react";
import { useRender } from "@base-ui/react/use-render";
import { Popover, PopoverContent, PopoverTrigger } from "./popover";
type Layout = "block" | "inline";
export type NavSwitcherProps = ComponentProps<typeof Popover> & {
    /** `block` fills its column (a sidebar, a pane) with a bordered 56 px box; `inline` sizes to its content (a header, a toolbar). */
    layout?: Layout;
    /** Whether a `block` trigger is the size container its `mark` and `tag` rules read (default `true`: it measures its own
     * box, which is about 2 rem narrower than the column around it). `false` creates no container: the rules then read the
     * nearest ancestor container named `nav-switcher`, so the host names its pane (`container: nav-switcher / inline-size`,
     * or `container-name: sidebar nav-switcher`) and the mark follows the pane's width. Without such an ancestor the
     * container rules do not apply and the mark stays. The thresholds are the pane's content width: below 17.5rem a
     * mark that has a `tag` beside it is hidden, below 15rem every mark is. */
    contain?: boolean;
};
/**
 * "Where am I": a switcher for the entity the rest of the screen is about (a mailbox, a workspace, a project). A
 * trigger that names the current one opens a popover with a list of LINKS, one per entity, so a middle click opens a
 * tab and the router owns navigation. It is a disclosure, not a menu: Tab moves through the links, Escape closes it
 * and focus goes back to the trigger.
 *
 * ```tsx
 * <NavSwitcher>
 *   <NavSwitcherTrigger label="Switch mailbox" mark={<Avatar …/>} title="ana@example.test" description="Personal" />
 *   <NavSwitcherContent label="Switch mailbox">
 *     <NavSwitcherItem render={<Link href="/a" />} current mark={…} tag={…}>ana@example.test</NavSwitcherItem>
 *     <NavSwitcherSeparator />
 *     <NavSwitcherItem render={<Link href="/link" />} mark={<Plus />}>Link a mailbox</NavSwitcherItem>
 *   </NavSwitcherContent>
 * </NavSwitcher>
 * ```
 */
export declare function NavSwitcher({ layout, contain, ...props }: NavSwitcherProps): import("react").JSX.Element;
export type NavSwitcherTriggerProps = Omit<ComponentProps<typeof PopoverTrigger>, "children" | "className"> & {
    className?: string;
    /** The current entity's mark: an avatar, a gem, an icon. Hidden from assistive technology. */
    mark?: ReactNode;
    /** The current entity's name. Ellipsized; user text, so it carries `dir="auto"`. */
    title: ReactNode;
    /** A second line (a plan, a detail, a status tag). */
    description?: ReactNode;
    /** After the text, before the chevrons: an attention count, a status tag. */
    tag?: ReactNode;
    /** Screen-reader words before the title ("Switch mailbox"): the button's name is "Switch mailbox: ana@example.test". */
    label?: string;
};
export declare function NavSwitcherTrigger({ mark, title, description, tag, label, className, ...props }: NavSwitcherTriggerProps): import("react").JSX.Element;
export type NavSwitcherContentProps = Omit<ComponentProps<typeof PopoverContent>, "children" | "aria-label"> & {
    /** The popover's name. */
    label: string;
    /** The link list's name (a `nav` landmark); the popover's name when omitted. */
    listLabel?: string;
    /** `NavSwitcherItem`s and `NavSwitcherSeparator`s. */
    children: ReactNode;
    /** Below the list and outside its scroll: a stale note, a retry. The list scrolls, this stays in view. */
    footer?: ReactNode;
};
/** The popover: a `nav` list of links that scrolls on its own, and a footer that does not. Focus opens on the current link, else the first. */
export declare function NavSwitcherContent({ label, listLabel, children, footer, className, ref, initialFocus, ...props }: NavSwitcherContentProps): import("react").JSX.Element;
export type NavSwitcherItemProps = useRender.ComponentProps<"a"> & {
    /** The current entity: `aria-current="page"`, the fill and the bar; a check mark at the end unless `tag` is given. */
    current?: boolean;
    /** Leading mark (an avatar, an icon). */
    mark?: ReactNode;
    /** A second line inside the link, part of its name: a status tag ("Needs authorization"). */
    description?: ReactNode;
    /** After the text: replaces the check mark of the current item. */
    tag?: ReactNode;
};
/** One link of the list. Pass the host's router link through `render`. The name wraps to two lines instead of being cut: touch has no tooltip. */
export declare function NavSwitcherItem({ current, mark, description, tag, children, ...props }: NavSwitcherItemProps): import("react").JSX.Element;
/** A hairline between groups of links (the footer action below the entities). An empty, hidden `li`: a list cannot hold a separator role. */
export declare function NavSwitcherSeparator({ className, ...props }: ComponentProps<"li">): import("react").JSX.Element;
export {};
