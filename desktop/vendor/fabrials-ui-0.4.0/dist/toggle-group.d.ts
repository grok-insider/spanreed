import type { ReactNode } from "react";
import { Toggle as BaseToggle } from "@base-ui/react/toggle";
import { ToggleGroup as BaseToggleGroup } from "@base-ui/react/toggle-group";
import { type StyledProps } from "./shared";
export declare function ToggleGroup({ className, size, ...props }: StyledProps<BaseToggleGroup.Props> & {
    size?: "default" | "sm";
}): import("react").JSX.Element;
export declare function ToggleGroupItem({ className, ...props }: StyledProps<BaseToggle.Props>): import("react").JSX.Element;
export type ThemePreference = "system" | "light" | "dark";
/**
 * Presentational theme choice. The host owns persistence and applies `.dark`.
 */
export declare function ThemeSwitcher({ value, onValueChange, labels, showLabels, label, className, }: {
    value: ThemePreference;
    onValueChange: (value: ThemePreference) => void;
    labels?: Record<ThemePreference, ReactNode>;
    showLabels?: boolean;
    label?: string;
    className?: string;
}): import("react").JSX.Element;
