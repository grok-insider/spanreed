import * as React from "react";
import type { PrivateRecentPage } from "./contracts";
export declare function PrivateHistoryView({ fetchPage, description, heading, }: {
    fetchPage: (before: number | null) => Promise<PrivateRecentPage>;
    description: string;
    /** The host already shows the page title. */
    heading?: boolean;
}): React.JSX.Element;
