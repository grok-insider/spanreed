import type { ComponentProps, ReactNode } from "react";
import { useRender } from "@base-ui/react/use-render";
import { ChevronDown } from "lucide-react";
import { Button } from "./controls";
export type AppHeaderProps = Omit<ComponentProps<"header">, "children"> & {
    /** The product mark: an `AppHeaderBrand` around a `ProductLockup` (or anything). It shrinks before the controls do. */
    brand: ReactNode;
    /** Primary navigation: an `AppHeaderNav` of `AppHeaderLink`s. It keeps its place on a phone (icons only). */
    navigation?: ReactNode;
    /** The command launcher (a palette trigger). Its slot has a definite width per mode, and is the size container `command` it answers to. */
    command?: ReactNode;
    /** Session chrome at the end of the row: settings, account, preferences. */
    actions?: ReactNode;
    /** Stays under the top of its scroll container. The workspace shell keeps the header put by itself, so this is for pages that flow. */
    sticky?: boolean;
};
/**
 * The application header: one row of 56 px with a hairline below, no blur, spanning the window with its content on
 * the page gutter. Order in the row: brand, navigation, then the command slot and the actions pushed to the end.
 * It carries session chrome only, never anything scoped to a mailbox, an account, a folder or a message.
 *
 * Presentational, no hooks. The modes are container queries in rem on the header's own width, so text zoom collapses
 * the header instead of overflowing it: compact below 48rem (labels become screen-reader text, controls are square),
 * medium to 72rem, wide from there (the command slot grows to 14, 18 and 22rem), the brand name gives way at 24rem
 * and the row wraps in two at 19rem. The command slot is definite in width, never only a `flex-basis`: Firefox and
 * WebKit size the actions cluster from its content, so a slot that only has a basis makes the cluster too narrow and
 * the header overflows.
 */
export declare function AppHeader({ brand, navigation, command, actions, sticky, className, ...props }: AppHeaderProps): import("react").JSX.Element;
/** The brand link: 44 px tall, it grows into the gutter so the mark itself starts on it. Render the host's router link through `render`. */
export declare function AppHeaderBrand({ render, className, ...props }: useRender.ComponentProps<"a">): import("react").ReactElement<unknown, string | import("react").JSXElementConstructor<any>>;
/** An installation's logo inside a `ProductLockup` (`mark={<AppHeaderLogo src=… width height />}`): sized by the lockup and shrinking on a phone. */
export declare function AppHeaderLogo({ className, alt, ...props }: ComponentProps<"img">): import("react").JSX.Element;
/** Primary navigation: a labelled `nav` of plain links. */
export declare function AppHeaderNav({ label, className, ...props }: Omit<ComponentProps<"nav">, "aria-label" | "aria-labelledby"> & {
    label: string;
}): import("react").JSX.Element;
/**
 * A navigation link: icon and text, full ink and a 2 px Stormlight bar on the header's hairline when `current`
 * (`aria-current="page"`). Put the text in a `span` (or `AppHeaderLabel`): below 48rem the icon stands alone.
 */
export declare function AppHeaderLink({ current, render, className, ...props }: useRender.ComponentProps<"a"> & {
    current?: boolean;
}): import("react").ReactElement<unknown, string | import("react").JSXElementConstructor<any>>;
/** A ghost `Button` of the header row (an account trigger, a preferences menu, a settings button). Below 48rem it becomes a square icon: its `AppHeaderLabel` turns into screen-reader text and its `AppHeaderCaret` goes. Base UI triggers can `render` it. */
export declare function AppHeaderAction({ className, variant, size, ...props }: ComponentProps<typeof Button>): import("react").JSX.Element;
/** Text that stays visible on wide headers and becomes screen-reader text below 48rem. It ellipsizes at 14rem. */
export declare function AppHeaderLabel({ className, ...props }: ComponentProps<"span">): import("react").JSX.Element;
/** The chevron of a menu trigger: gone when the header is compact. */
export declare function AppHeaderCaret({ className, ...props }: Omit<ComponentProps<typeof ChevronDown>, "aria-hidden">): import("react").JSX.Element;
/** The box of a control that is not there yet (a trigger whose boundary is still resolving), so nothing shifts when it arrives. */
export declare function AppHeaderPlaceholder({ className, ...props }: ComponentProps<"span">): import("react").JSX.Element;
