export type RecencyBucket = "today" | "yesterday" | "week" | "month" | "older";
export declare const RECENCY_LABELS: Readonly<Record<RecencyBucket, string>>;
export type RecencyGroup<T> = {
    key: string;
    bucket: RecencyBucket;
    label: string;
    items: T[];
};
export type RecencyOptions = {
    timeZone?: string;
    locale?: string;
    labels?: Partial<Record<RecencyBucket, string>>;
};
export type RecencyDate = string | number | Date;
export declare function recencyGroupFor(value: RecencyDate, now?: Date, options?: RecencyOptions): {
    key: string;
    bucket: RecencyBucket;
    label: string;
};
export declare function groupByRecency<T>(items: readonly T[], getDate: (item: T) => RecencyDate, now?: Date, options?: RecencyOptions): RecencyGroup<T>[];
