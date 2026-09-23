import * as React from "react";
import type { LocalUsageSnapshotV2 } from "./contracts";
export type ConsumptionSnapshot = LocalUsageSnapshotV2;
export declare function SynchronizedConsumption({ load, }: {
    load: () => Promise<{
        snapshots: ConsumptionSnapshot[];
    }>;
}): React.JSX.Element;
