import * as React from "react";
export declare function RoutingExplanation(): React.JSX.Element;
export declare function RoutingPolicyForm({ provider, enabled, threshold, pending, onSave, }: {
    provider: string;
    enabled: boolean;
    threshold: number;
    pending: boolean;
    onSave: (enabled: boolean, threshold: number) => Promise<void>;
}): React.JSX.Element;
import type { Observation, CreditBalance, ResetInventory as ResetCredits, ProviderOutput } from "./contracts";
export declare function useClock(): number | null;
export declare function ResetInventory({ observation, }: {
    observation?: Observation<ResetCredits> | null;
}): React.JSX.Element | null;
export declare function ObservationStatus({ observation, }: {
    observation: Observation<unknown>;
}): React.JSX.Element;
export declare function BalanceCard({ observation, }: {
    observation?: Observation<CreditBalance> | null;
}): React.JSX.Element | null;
export declare function ProviderCard({ provider }: {
    provider: ProviderOutput;
}): React.JSX.Element;
