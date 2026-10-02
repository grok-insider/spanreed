import { type ComponentProps } from "react";
import { useRender } from "@base-ui/react/use-render";
/**
 * A list of records: a `ul` with no bullets and no padding. It is the size container the rows answer to
 * (`@container fui-item-group`), so a row in a narrow pane and a row in a wide one use the same markup.
 * `bordered` adds a hairline above the first row; every row carries its own hairline below, so the list is
 * closed at the bottom without doubling.
 */
export declare function ItemGroup({ className, bordered, ...props }: ComponentProps<"ul"> & {
    bordered?: boolean;
}): import("react").JSX.Element;
export type ItemProps = useRender.ComponentProps<"li"> & {
    /** `default` is a row of a list. `outline` and `muted` are standalone items (a boxed row, a quiet well). */
    variant?: "default" | "outline" | "muted";
    size?: "default" | "sm";
    /** The record that is open in the detail pane: a soft Stormlight fill and a 2 px bar on the inline-start edge. The link inside says the same with `aria-current`. */
    current?: boolean;
    /** Picked for a bulk action: the same fill, no bar. A checked native checkbox inside an `ItemCheck` does it without this prop. */
    selected?: boolean;
    /** Not yet read: the title takes weight 600 (add `ItemUnread` and a screen-reader word inside the link). */
    unread?: boolean;
    /** The whole row is the target of the link or button in its title (`ItemLink` or `ItemTitle` with a link): it stretches over the row, and hover paints the row. */
    stretch?: boolean;
};
/**
 * One record. Inside an `ItemGroup` it is a `li`; alone it is a `div`. The root carries only the row: a 44 px minimum,
 * a hairline below, hover, current and selected fills. With `ItemMedia`, `ItemContent` and `ItemActions` as children
 * it also lays them out in a row; without them (a host that draws its own grid inside) it adds nothing.
 */
export declare function Item({ className, variant, size, current, selected, unread, stretch, render, ...props }: ItemProps): import("react").ReactElement<unknown, string | import("react").JSXElementConstructor<any>>;
export type ItemLinkProps = useRender.ComponentProps<"a"> & {
    /** `true` is `aria-current="true"` (the current item of a set that is not a page); `"page"` is a page of the site. */
    current?: boolean | "page" | "true";
};
/**
 * The link (or, with `render={<button />}`, the button) that is the row's target. Inside an `Item` with `stretch`
 * it covers the whole row; its focus ring is drawn inside the row so a scrolling parent never clips it. Pass the
 * host's router link through `render`.
 */
export declare function ItemLink({ current, render, className, ...props }: ItemLinkProps): import("react").ReactElement<unknown, string | import("react").JSXElementConstructor<any>>;
/** Leading visual: an icon in a small box (`variant="icon"`), an image or avatar (`variant="image"`), or bare. */
export declare function ItemMedia({ variant, className, render, ...props }: useRender.ComponentProps<"div"> & {
    variant?: "default" | "icon" | "image";
}): import("react").ReactElement<unknown, string | import("react").JSXElementConstructor<any>>;
/** The text column. It never takes `position`, `transform` or `contain`, so the row's stretched link keeps the row as its containing block. */
export declare function ItemContent({ className, render, ...props }: useRender.ComponentProps<"div">): import("react").ReactElement<unknown, string | import("react").JSXElementConstructor<any>>;
/** The record's name. Give it a heading with `render={<h3 />}`; a link inside it becomes the stretched target. */
export declare function ItemTitle({ className, render, ...props }: useRender.ComponentProps<"div">): import("react").ReactElement<unknown, string | import("react").JSXElementConstructor<any>>;
/** A second line, muted. Two lines at most, then an ellipsis. */
export declare function ItemDescription({ className, render, ...props }: useRender.ComponentProps<"p">): import("react").ReactElement<unknown, string | import("react").JSXElementConstructor<any>>;
/** Controls of the row (a star, a menu): they sit above the stretched target, so they stay separate targets. */
export declare function ItemActions({ className, render, ...props }: useRender.ComponentProps<"div">): import("react").ReactElement<unknown, string | import("react").JSXElementConstructor<any>>;
/** A bare control that must stay a target of its own, above the stretched one (a star inside a row that lays itself out): `position` and `z-index` and nothing else. `ItemActions` is the same with a layout. */
export declare function ItemControl({ className, render, ...props }: useRender.ComponentProps<"span">): import("react").ReactElement<unknown, string | import("react").JSXElementConstructor<any>>;
/** A full-width line above the content (shadcn name). */
export declare function ItemHeader({ className, render, ...props }: useRender.ComponentProps<"div">): import("react").ReactElement<unknown, string | import("react").JSXElementConstructor<any>>;
/** A full-width line below the content (shadcn name). */
export declare function ItemFooter({ className, render, ...props }: useRender.ComponentProps<"div">): import("react").ReactElement<unknown, string | import("react").JSXElementConstructor<any>>;
/** The 44 by 44 label of a row's checkbox: the whole square is the target and the native box stays 16 px. Above the stretched target. */
export declare function ItemCheck({ className, render, ...props }: useRender.ComponentProps<"label">): import("react").ReactElement<unknown, string | import("react").JSXElementConstructor<any>>;
/** The unread marker: a 6 px square in ink (never Stormlight). Decorative: say "unread" in the link's text for screen readers. */
export declare function ItemUnread({ className, render, ...props }: useRender.ComponentProps<"span">): import("react").ReactElement<unknown, string | import("react").JSXElementConstructor<any>>;
/**
 * A hairline between items where they should not each carry their own (shadcn name). Inside a group it is an empty,
 * hidden `li` (a `role="separator"` child is not allowed in a list); alone it is a separator.
 */
export declare function ItemSeparator({ className, ...props }: ComponentProps<"li">): import("react").JSX.Element;
