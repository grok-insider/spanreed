import * as React from "react";
import type { PrivateRecentPage } from "./contracts";
export declare function PrivateHistoryView({ fetchPage, description, }: {
    fetchPage: (before: number | null) => Promise<PrivateRecentPage>;
    description: string;
}): React.JSX.Element;
