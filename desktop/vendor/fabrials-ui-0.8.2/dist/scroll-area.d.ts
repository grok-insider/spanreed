import { ScrollArea as ScrollAreaPrimitive } from "@base-ui/react/scroll-area";
declare function ScrollArea({ className, children, ...props }: Omit<ScrollAreaPrimitive.Root.Props, "className"> & {
    className?: string;
}): import("react").JSX.Element;
declare function ScrollBar({ className, orientation, ...props }: Omit<ScrollAreaPrimitive.Scrollbar.Props, "className"> & {
    className?: string;
}): import("react").JSX.Element;
export { ScrollArea, ScrollBar };
