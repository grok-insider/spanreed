import type { ComponentProps, ReactNode } from "react";
import type { Tone } from "./display";
export declare function Timeline({ className, ...props }: ComponentProps<"ol">): import("react").JSX.Element;
export type TimelineItemProps = Omit<ComponentProps<"li">, "title"> & {
    /** A small icon or avatar for the marker. */
    icon?: ReactNode;
    tone?: Tone;
    title: ReactNode;
    /** Usually a RelativeTime. */
    time?: ReactNode;
    /** Controls at the end of the header row. */
    actions?: ReactNode;
    /** Arrived since the list was first shown: highlighted once, never looping. */
    fresh?: boolean;
};
/**
 * One event in a vertical feed: a marker on a rail, a header with the time,
 * and optional detail. Tone colours the marker; the title carries the meaning.
 */
export declare function TimelineItem({ icon, tone, title, time, actions, fresh, children, className, ...props }: TimelineItemProps): import("react").JSX.Element;
