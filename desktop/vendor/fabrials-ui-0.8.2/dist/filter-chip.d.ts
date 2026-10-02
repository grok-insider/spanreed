import type { ComponentProps } from "react";
import { useRender } from "@base-ui/react/use-render";
type FilterChipBase = {
    label: string;
    value: string;
    clearLabel?: string;
    className?: string;
};
export type FilterChipLinkProps = FilterChipBase & Omit<useRender.ComponentProps<"a">, "children" | "className"> & {
    href?: string;
    onRemove?: undefined;
};
export type FilterChipButtonProps = FilterChipBase & Omit<ComponentProps<"button">, "children" | "className" | "onClick" | "type" | "value"> & {
    onRemove: () => void;
    href?: undefined;
    render?: undefined;
};
export type FilterChipProps = FilterChipLinkProps | FilterChipButtonProps;
export declare function FilterChip(props: FilterChipProps): import("react").JSX.Element;
export {};
