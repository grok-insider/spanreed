import * as React from "react";
import { useRender } from "@base-ui/react/use-render";
import { Button, Input } from "./controls";
import { Separator } from "./display";
import { TooltipContent } from "./menu";
type SidebarContextProps = {
    state: "expanded" | "collapsed";
    open: boolean;
    setOpen: (open: boolean) => void;
    openMobile: boolean;
    setOpenMobile: (open: boolean) => void;
    isMobile: boolean;
    toggleSidebar: () => void;
};
declare function useSidebar(): SidebarContextProps;
declare function SidebarProvider({ defaultOpen, open: openProp, onOpenChange: setOpenProp, keyboardShortcut, persist, className, style, children, ...props }: React.ComponentProps<"div"> & {
    defaultOpen?: boolean;
    open?: boolean;
    onOpenChange?: (open: boolean) => void;
    /** The key that toggles the sidebar with Ctrl or Command (default `"b"`), or `false` for none. It is ignored while a person types in a field or an editor. */
    keyboardShortcut?: string | false;
    /** Remember the state in the `sidebar_state` cookie (default). `false` writes nothing: use it when the host keeps the state. */
    persist?: boolean;
}): React.JSX.Element;
declare function Sidebar({ side, variant, collapsible, labels, className, children, dir, ...props }: React.ComponentProps<"div"> & {
    side?: "left" | "right";
    variant?: "sidebar" | "floating" | "inset";
    collapsible?: "offcanvas" | "icon" | "none";
    /** The name and description the phone sheet announces (screen-reader text, English by default). */
    labels?: {
        title: string;
        description: string;
    };
}): React.JSX.Element;
declare function SidebarTrigger({ className, onClick, children, label, ...props }: React.ComponentProps<typeof Button> & {
    /** The button's name (screen-reader text). */
    label?: string;
}): React.JSX.Element;
declare function SidebarRail({ className, label, ...props }: React.ComponentProps<"button"> & {
    /** The rail's name and tooltip. */
    label?: string;
}): React.JSX.Element;
declare function SidebarInset({ className, ...props }: React.ComponentProps<"main">): React.JSX.Element;
declare function SidebarInput({ className, ...props }: React.ComponentProps<typeof Input>): React.JSX.Element;
declare function SidebarHeader({ className, ...props }: React.ComponentProps<"div">): React.JSX.Element;
declare function SidebarFooter({ className, ...props }: React.ComponentProps<"div">): React.JSX.Element;
declare function SidebarSeparator({ className, ...props }: React.ComponentProps<typeof Separator>): React.JSX.Element;
declare function SidebarContent({ className, ...props }: React.ComponentProps<"div">): React.JSX.Element;
declare function SidebarGroup({ className, ...props }: React.ComponentProps<"div">): React.JSX.Element;
declare function SidebarGroupLabel({ className, render, ...props }: useRender.ComponentProps<"div"> & React.ComponentProps<"div">): React.ReactElement<unknown, string | React.JSXElementConstructor<any>>;
declare function SidebarGroupAction({ className, render, ...props }: useRender.ComponentProps<"button"> & React.ComponentProps<"button">): React.ReactElement<unknown, string | React.JSXElementConstructor<any>>;
declare function SidebarGroupContent({ className, ...props }: React.ComponentProps<"div">): React.JSX.Element;
declare function SidebarMenu({ className, ...props }: React.ComponentProps<"ul">): React.JSX.Element;
declare function SidebarMenuItem({ className, ...props }: React.ComponentProps<"li">): React.JSX.Element;
declare function SidebarMenuButton({ render, isActive, variant, size, depth, tooltip, className, style, ...props }: useRender.ComponentProps<"button"> & React.ComponentProps<"button"> & {
    isActive?: boolean;
    variant?: "default" | "outline";
    /** `touch` is a navigation row: a 44 px target around a 40 px band, a 2 px Stormlight bar when active, the focus ring inside the band. */
    size?: "default" | "sm" | "lg" | "touch";
    /** Indent of a nested row (folders in a tree), 0 to 4 steps of 0.75rem, in `touch` rows. */
    depth?: number;
    tooltip?: string | React.ComponentProps<typeof TooltipContent>;
}): React.JSX.Element;
declare function SidebarMenuAction({ className, render, showOnHover, ...props }: useRender.ComponentProps<"button"> & React.ComponentProps<"button"> & {
    showOnHover?: boolean;
}): React.ReactElement<unknown, string | React.JSXElementConstructor<any>>;
/**
 * A count at the end of a row. As a sibling of the menu button it floats over the button's end; INSIDE the button
 * (a `span`, so it is valid in a link or a button) it takes its place in the row and is part of the link's accessible
 * name: a screen reader hears "Inbox 12".
 */
declare function SidebarMenuBadge({ className, ...props }: React.ComponentProps<"span">): React.JSX.Element;
declare function SidebarMenuSkeleton({ className, showIcon, ...props }: React.ComponentProps<"div"> & {
    showIcon?: boolean;
}): React.JSX.Element;
declare function SidebarMenuSub({ className, ...props }: React.ComponentProps<"ul">): React.JSX.Element;
declare function SidebarMenuSubItem({ className, ...props }: React.ComponentProps<"li">): React.JSX.Element;
declare function SidebarMenuSubButton({ render, size, isActive, className, ...props }: useRender.ComponentProps<"a"> & React.ComponentProps<"a"> & {
    size?: "sm" | "md";
    isActive?: boolean;
}): React.ReactElement<unknown, string | React.JSXElementConstructor<any>>;
export { Sidebar, SidebarContent, SidebarFooter, SidebarGroup, SidebarGroupAction, SidebarGroupContent, SidebarGroupLabel, SidebarHeader, SidebarInput, SidebarInset, SidebarMenu, SidebarMenuAction, SidebarMenuBadge, SidebarMenuButton, SidebarMenuItem, SidebarMenuSkeleton, SidebarMenuSub, SidebarMenuSubButton, SidebarMenuSubItem, SidebarProvider, SidebarRail, SidebarSeparator, SidebarTrigger, useSidebar, };
