export type NumberTickerProps = {
    value: number;
    pad?: number;
    duration?: number;
    stagger?: number;
    startOnView?: boolean;
    prefix?: string;
    suffix?: string;
    blur?: boolean;
    className?: string;
    digitClassName?: string;
    locale?: boolean;
    format?: (value: number) => string;
};
export declare function NumberTicker({ value, pad, duration, stagger, startOnView, prefix, suffix, blur, className, digitClassName, locale, format, }: NumberTickerProps): import("react").JSX.Element;
