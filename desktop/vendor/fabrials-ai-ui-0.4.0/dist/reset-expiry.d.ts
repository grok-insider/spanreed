import type { Observation, ResetInventory } from "./contracts";
export declare function validTimestamp(value: number | null | undefined): value is number;
export declare function resetExpiryLabel(expiry: number | null | undefined, now: number | null): string;
export declare function upcomingResetExpiries(inventory: ResetInventory | null, now: number | null): number[];
export declare function resetExpirySummary(observation: Observation<ResetInventory>, now: number): {
    stale: boolean;
    expiring: number;
    expired: number;
    nextExpiry: number | null;
};
