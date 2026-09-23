import type { ComponentProps } from "react";
import { Tabs as BaseTabs } from "@base-ui/react/tabs";
import { type StyledProps } from "./shared";
export type Tone = "neutral" | "info" | "success" | "warning" | "danger";
export declare function Badge({ className, tone, variant, dot, children, ...props }: ComponentProps<"span"> & {
    tone?: Tone | "accent";
    variant?: "soft" | "outline" | "solid";
    /** Adds a leading status dot; the text remains the accessible meaning. */
    dot?: boolean;
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
export declare function Alert({ className, variant, ...props }: ComponentProps<"div"> & {
    variant?: "default" | "info" | "success" | "warning" | "destructive";
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
export declare const Tabs: import("react").ForwardRefExoticComponent<Omit<import("@base-ui/react").TabsRootProps, "ref"> & import("react").RefAttributes<HTMLDivElement>>;
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
export declare function TabsList({ className, variant, ...props }: StyledProps<BaseTabs.List.Props> & {
    variant?: "underline" | "segmented";
}): import("react").JSX.Element;
export declare function TabsTrigger({ className, ...props }: StyledProps<BaseTabs.Tab.Props>): import("react").JSX.Element;
export declare function TabsContent({ className, ...props }: StyledProps<BaseTabs.Panel.Props>): import("react").JSX.Element;
