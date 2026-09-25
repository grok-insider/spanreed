export type ChartSeries = {
    key: string;
    label?: string;
    color?: string;
    dashed?: boolean;
};
export type SeriesChartProps = {
    data: Array<Record<string, string | number | null | undefined>>;
    xKey: string;
    series: ChartSeries[];
    kind?: "bar" | "line";
    stacked?: boolean;
    lineType?: "monotone" | "step";
    yFormat?: (value: number) => string;
    titleKey?: string;
    caption: string;
    height?: number;
    className?: string;
};
export declare function SeriesChart({ data, xKey, series, kind, stacked, lineType, yFormat, titleKey, caption, height, className, }: SeriesChartProps): import("react").JSX.Element;
