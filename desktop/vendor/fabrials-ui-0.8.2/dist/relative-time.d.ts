import { type ComponentProps } from "react";
export type RelativeTimeStyle = "long" | "short" | "narrow";
/**
 * "3 minutes ago" in the given locale. Under 45 seconds reads as "now";
 * beyond `absoluteAfterDays` it becomes a date.
 */
export declare function formatRelativeTime(date: Date | string | number, now: number, locale: string, opts?: {
    style?: RelativeTimeStyle;
    absoluteAfterDays?: number;
    timeZone?: string;
}): string;
/**
 * Options for `Intl.DateTimeFormat`, or a function that picks them per date: a row that drops the year when it is this
 * year needs the clock (`now`) to decide, and the caller owns that rule.
 */
export type AbsoluteTimeFormat = Intl.DateTimeFormatOptions | ((date: Date, now: number) => Intl.DateTimeFormatOptions);
/**
 * A date as an absolute, deterministic string ("29 Sept, 09:41"). It reads no clock unless the format is a function,
 * which receives `opts.now` (default: the current time). Returns "" for an invalid date. Pass a `timeZone` (in the
 * format or in `opts`) for a string that is the same on the server and in the browser.
 */
export declare function formatAbsoluteTime(date: Date | string | number, locale: string, format: AbsoluteTimeFormat, opts?: {
    now?: number;
    timeZone?: string;
}): string;
export type RelativeTimeProps = Omit<ComponentProps<"time">, "children" | "dateTime"> & {
    date: Date | string | number;
    locale?: string;
    style?: RelativeTimeStyle;
    absoluteAfterDays?: number;
    /**
     * Deterministic mode: print the date with these `Intl.DateTimeFormat` options (or a function that picks them per date)
     * instead of "3 minutes ago". No timer runs, so a list renders the same tomorrow and in a screenshot.
     */
    absoluteFormat?: AbsoluteTimeFormat;
    /**
     * The clock. A number is a fixed instant and stops the timer (tests, screenshots, a server-anchored render); a function
     * is read at every tick (a clock anchored to the server's time). Default: the device clock.
     */
    now?: number | (() => number);
    /**
     * The zone dates are printed in, also in the `title` and in the absolute fallback of a far date. Without it the
     * browser's zone is used, which differs from the server's: it is REQUIRED whenever a deterministic time (`absoluteFormat`
     * or a fixed `now`) is server-rendered, or React reports a mismatch (the title is zone-dependent even when the text is not).
     */
    timeZone?: string;
};
/**
 * A `<time>` that reads "3 minutes ago" and keeps itself current while the page is visible. The full date is in the
 * title. With `absoluteFormat` or a fixed `now` it is deterministic and does not tick; give it a `timeZone` and the
 * server and the browser print the same text (hydration-exact, no warning suppressed).
 */
export declare function RelativeTime({ date, locale, style, absoluteAfterDays, absoluteFormat, now, timeZone, className, title, ...props }: RelativeTimeProps): import("react").JSX.Element;
