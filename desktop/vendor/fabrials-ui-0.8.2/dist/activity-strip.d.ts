import type { ComponentProps, ReactNode } from "react";
import type { Tone } from "./display";
export type ActivityCell = {
    /** What the cell stands for, e.g. a day or an hour. */
    label: string;
    value: number;
    /** Colours this cell with a status tone instead of the brand ramp. */
    tone?: Tone;
};
export type ActivityStripProps = Omit<ComponentProps<"figure">, "children"> & {
    cells: ReadonlyArray<ActivityCell>;
    /** Accessible summary; also the caption of the screen-reader table. */
    caption: string;
    /** Value of the fullest cell. Defaults to the largest value. */
    max?: number;
    size?: "sm" | "md" | "lg";
    /** Text under the first and last cell, e.g. "hace 90 días" / "hoy". */
    startLabel?: ReactNode;
    endLabel?: ReactNode;
    formatValue?: (value: number) => string;
};
/** 0 for no activity, then 1–4 by share of `max`. */
export declare function activityLevel(value: number, max: number): 0 | 1 | 2 | 3 | 4;
/**
 * A row of intensity cells, like an uptime bar or a row of contributions:
 * one cell per day, hour or bucket. Colour is backed by a native tooltip and
 * a screen-reader table with every value.
 */
export declare function ActivityStrip({ cells, caption, max, size, startLabel, endLabel, formatValue, className, ...props }: ActivityStripProps): import("react").JSX.Element;
