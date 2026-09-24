import type * as React from "react";
import { NavigationMenu as NavigationMenuPrimitive } from "@base-ui/react/navigation-menu";
type Styled<T> = Omit<T, "className"> & {
    className?: string;
};
declare function NavigationMenu({ align, className, children, ...props }: Styled<NavigationMenuPrimitive.Root.Props> & Pick<NavigationMenuPrimitive.Positioner.Props, "align">): React.JSX.Element;
declare function NavigationMenuList({ className, ...props }: Styled<React.ComponentPropsWithRef<typeof NavigationMenuPrimitive.List>>): React.JSX.Element;
declare function NavigationMenuItem({ className, ...props }: Styled<React.ComponentPropsWithRef<typeof NavigationMenuPrimitive.Item>>): React.JSX.Element;
declare function navigationMenuTriggerStyle(options?: {
    className?: string;
}): string;
declare function NavigationMenuTrigger({ className, children, ...props }: Styled<NavigationMenuPrimitive.Trigger.Props>): React.JSX.Element;
declare function NavigationMenuContent({ className, ...props }: Styled<NavigationMenuPrimitive.Content.Props>): React.JSX.Element;
declare function NavigationMenuPositioner({ className, side, sideOffset, align, alignOffset, ...props }: Styled<NavigationMenuPrimitive.Positioner.Props>): React.JSX.Element;
declare function NavigationMenuLink({ className, ...props }: Styled<NavigationMenuPrimitive.Link.Props>): React.JSX.Element;
declare function NavigationMenuIndicator({ className, ...props }: Styled<React.ComponentPropsWithRef<typeof NavigationMenuPrimitive.Icon>>): React.JSX.Element;
export { NavigationMenu, NavigationMenuContent, NavigationMenuIndicator, NavigationMenuItem, NavigationMenuLink, NavigationMenuList, NavigationMenuTrigger, navigationMenuTriggerStyle, NavigationMenuPositioner, };
