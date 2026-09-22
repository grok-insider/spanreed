import { Menu as BaseMenu } from "@base-ui/react/menu";
import { Tooltip as BaseTooltip } from "@base-ui/react/tooltip";
import { type StyledProps } from "./shared";
export declare const DropdownMenu: <Payload>(props: BaseMenu.Root.Props<Payload>) => import("react").JSX.Element;
export declare const DropdownMenuTrigger: BaseMenu.Trigger;
export declare const DropdownMenuGroup: import("react").ForwardRefExoticComponent<Omit<import("@base-ui/react").ContextMenuGroupProps, "ref"> & import("react").RefAttributes<HTMLDivElement>>;
export declare function DropdownMenuContent({ className, align, sideOffset, ...props }: StyledProps<BaseMenu.Popup.Props> & Pick<BaseMenu.Positioner.Props, "align" | "sideOffset">): import("react").JSX.Element;
export declare function DropdownMenuItem({ className, destructive, ...props }: StyledProps<BaseMenu.Item.Props> & {
    destructive?: boolean;
}): import("react").JSX.Element;
export declare function DropdownMenuLabel({ className, ...props }: StyledProps<BaseMenu.GroupLabel.Props>): import("react").JSX.Element;
export declare function DropdownMenuSeparator({ className, ...props }: StyledProps<BaseMenu.Separator.Props>): import("react").JSX.Element;
export declare const TooltipProvider: import("react").FC<import("@base-ui/react").TooltipProviderProps>;
export declare const Tooltip: <Payload>(props: BaseTooltip.Root.Props<Payload>) => import("react").JSX.Element;
export declare const TooltipTrigger: BaseTooltip.Trigger;
export declare function TooltipContent({ className, sideOffset, side, align, ...props }: StyledProps<BaseTooltip.Popup.Props> & Pick<BaseTooltip.Positioner.Props, "sideOffset" | "side" | "align">): import("react").JSX.Element;
