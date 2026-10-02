import type { ReactNode } from "react";
import { Toggle as BaseToggle } from "@base-ui/react/toggle";
import { ToggleGroup as BaseToggleGroup } from "@base-ui/react/toggle-group";
import { type StyledProps } from "./shared";
export declare function ToggleGroup({ className, size, ...props }: StyledProps<BaseToggleGroup.Props> & {
    /** `sm` 26 px, `default` 34 px, `lg` 44 px painted on every pointer (the large control height); `sm` and `default` are 44 px on touch and narrow screens. */
    size?: "default" | "sm" | "lg";
}): import("react").JSX.Element;
export declare function ToggleGroupItem({ className, ...props }: StyledProps<BaseToggle.Props>): import("react").JSX.Element;
export type ThemePreference = "system" | "light" | "dark";
/**
 * Presentational theme choice. The host owns persistence and applies `.dark`.
 */
export declare function ThemeSwitcher({ value, onValueChange, labels, showLabels, label, size, className, }: {
    value: ThemePreference;
    onValueChange: (value: ThemePreference) => void;
    labels?: Record<ThemePreference, ReactNode>;
    showLabels?: boolean;
    label?: string;
    /** As `ToggleGroup`; `lg` paints 44 px on every pointer, for a row of primary 44 px targets. */
    size?: "default" | "sm" | "lg";
    className?: string;
}): import("react").JSX.Element;
