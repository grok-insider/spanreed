import type { SynchronizedAccount } from "./contracts";
export declare function SynchronizedAccounts({ accounts: allAccounts, includeLinked, }: {
    accounts: SynchronizedAccount[];
    includeLinked?: boolean;
}): import("react").JSX.Element | null;
export declare function LinkedAccountUsage({ accounts, }: {
    accounts: SynchronizedAccount[];
}): import("react").JSX.Element | null;
