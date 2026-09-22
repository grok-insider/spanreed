import * as React from "react";
import type { ConsumptionReport as Report, SourceStatus } from "./contracts";
export type { ConsumptionTotal } from "./contracts";
export type ConsumptionSource = SourceStatus;
export type ConsumptionReport = Omit<Report, "records" | "records_truncated">;
export declare function ConsumptionView({ report, synchronized, }: {
    report: ConsumptionReport;
    synchronized?: boolean;
}): React.JSX.Element;
