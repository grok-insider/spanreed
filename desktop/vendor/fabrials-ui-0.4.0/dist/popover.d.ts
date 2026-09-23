import * as React from "react";
import { Popover as PopoverPrimitive } from "@base-ui/react/popover";
declare function Popover({ ...props }: PopoverPrimitive.Root.Props): React.JSX.Element;
declare function PopoverTrigger({ ...props }: PopoverPrimitive.Trigger.Props): React.JSX.Element;
declare function PopoverContent({ className, align, alignOffset, side, sideOffset, ...props }: Omit<PopoverPrimitive.Popup.Props, "className"> & {
    className?: string;
} & Pick<PopoverPrimitive.Positioner.Props, "align" | "alignOffset" | "side" | "sideOffset">): React.JSX.Element;
declare function PopoverHeader({ className, ...props }: React.ComponentProps<"div">): React.JSX.Element;
declare function PopoverTitle({ className, ...props }: Omit<PopoverPrimitive.Title.Props, "className"> & {
    className?: string;
}): React.JSX.Element;
declare function PopoverDescription({ className, ...props }: Omit<PopoverPrimitive.Description.Props, "className"> & {
    className?: string;
}): React.JSX.Element;
export { Popover, PopoverContent, PopoverDescription, PopoverHeader, PopoverTitle, PopoverTrigger, };
