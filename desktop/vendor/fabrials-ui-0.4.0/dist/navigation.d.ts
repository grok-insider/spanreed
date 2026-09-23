import type { ComponentProps } from "react";
import { useRender } from "@base-ui/react/use-render";
export declare function Breadcrumb({ className, "aria-label": label, ...props }: ComponentProps<"nav">): import("react").JSX.Element;
export declare function BreadcrumbList({ className, ...props }: ComponentProps<"ol">): import("react").JSX.Element;
export declare function BreadcrumbItem({ className, ...props }: ComponentProps<"li">): import("react").JSX.Element;
export declare function BreadcrumbLink({ className, render, ...props }: useRender.ComponentProps<"a">): import("react").ReactElement<unknown, string | import("react").JSXElementConstructor<any>>;
export declare function BreadcrumbPage({ className, ...props }: ComponentProps<"span">): import("react").JSX.Element;
export declare function BreadcrumbSeparator({ className, children, ...props }: ComponentProps<"li">): import("react").JSX.Element;
export declare function BreadcrumbEllipsis({ className, label, ...props }: ComponentProps<"span"> & {
    label?: string;
}): import("react").JSX.Element;
export declare function Pagination({ className, "aria-label": label, ...props }: ComponentProps<"nav">): import("react").JSX.Element;
export declare function PaginationContent({ className, ...props }: ComponentProps<"ul">): import("react").JSX.Element;
export declare function PaginationItem(props: ComponentProps<"li">): import("react").JSX.Element;
export declare function PaginationLink({ className, isActive, render, ...props }: useRender.ComponentProps<"a"> & {
    isActive?: boolean;
}): import("react").ReactElement<unknown, string | import("react").JSXElementConstructor<any>>;
export declare function PaginationPrevious({ className, label, ...props }: useRender.ComponentProps<"a"> & {
    label?: string;
}): import("react").JSX.Element;
export declare function PaginationNext({ className, label, ...props }: useRender.ComponentProps<"a"> & {
    label?: string;
}): import("react").JSX.Element;
export declare function PaginationEllipsis({ className, label, ...props }: ComponentProps<"span"> & {
    label?: string;
}): import("react").JSX.Element;
