import type { ComponentProps, ReactNode } from "react";
import { Meter as BaseMeter } from "@base-ui/react/meter";
import type { Tone } from "./display";
export type Trend = "up" | "down" | "flat";
export type StatProps = Omit<ComponentProps<"div">, "children"> & {
    label: ReactNode;
    value: ReactNode;
    unit?: ReactNode;
    /** Change against the comparison period, e.g. "+12%". */
    delta?: ReactNode;
    trend?: Trend;
    /** Whether the trend is good news. Defaults to up = positive. */
    deltaTone?: "positive" | "negative" | "neutral";
    hint?: ReactNode;
    sparkline?: ReadonlyArray<number>;
    loading?: boolean;
};
export declare function Stat({ label, value, unit, delta, trend, deltaTone, hint, sparkline, loading, className, ...props }: StatProps): import("react").JSX.Element;
export declare function StatGroup({ className, ...props }: ComponentProps<"div">): import("react").JSX.Element;
export declare function Sparkline({ data, label, color, className, ...props }: Omit<ComponentProps<"svg">, "children"> & {
    data: ReadonlyArray<number>;
    /** Accessible summary. Without it the sparkline is decorative. */
    label?: string;
    color?: string;
}): import("react").JSX.Element;
export type MeterProps = Omit<BaseMeter.Root.Props, "className" | "children"> & {
    className?: string;
    label?: ReactNode;
    hint?: ReactNode;
    tone?: Tone;
    /** Automatic tone thresholds as a percentage of the range. */
    warnAt?: number;
    dangerAt?: number;
    showValue?: boolean;
};
export declare function Meter({ className, label, hint, tone, warnAt, dangerAt, showValue, value, min, max, ...props }: MeterProps): import("react").JSX.Element;
export declare function StatusDot({ tone, label, hideLabel, pulse, className, ...props }: Omit<ComponentProps<"span">, "children"> & {
    tone?: Tone;
    /** Text companion for the color. Required; hide it visually with hideLabel. */
    label: ReactNode;
    hideLabel?: boolean;
    pulse?: boolean;
}): import("react").JSX.Element;
export declare function DescriptionList({ className, layout, ...props }: ComponentProps<"dl"> & {
    layout?: "grid" | "stacked";
}): import("react").JSX.Element;
export declare function DescriptionItem({ className, ...props }: ComponentProps<"div">): import("react").JSX.Element;
export declare function DescriptionTerm({ className, ...props }: ComponentProps<"dt">): import("react").JSX.Element;
export declare function DescriptionDetails({ className, ...props }: ComponentProps<"dd">): import("react").JSX.Element;
