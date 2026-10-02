import type { ComponentProps } from "react";
/**
 * A size as people read it: 1024 steps and short unit labels in the given locale ("25 MB", "2,4 MB"); a whole number
 * for bytes and from 100 up, one decimal otherwise. Throws a `RangeError` for a negative or non-finite count: use
 * `FileSize` when the value may be unknown.
 */
export declare function formatBytes(locale: string, bytes: number): string;
/**
 * A `formatBytes` size as a tabular, unbroken span, with the exact count in the `title`. Rendering never throws:
 * an unknown or invalid size shows `fallback` (nothing by default).
 */
export declare function FileSize({ bytes, locale, fallback, className, title, ...props }: Omit<ComponentProps<"span">, "children"> & {
    bytes: number | null | undefined;
    locale?: string;
    fallback?: ComponentProps<"span">["children"];
}): import("react").JSX.Element | null;
