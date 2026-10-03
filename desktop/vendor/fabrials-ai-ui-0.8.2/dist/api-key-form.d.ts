import * as React from "react";
export interface ApiKeyEnrollment {
    provider: string;
    alias: string;
    key: string;
}
export declare function ApiKeyFields({ provider, alias, secret, onProviderChange, onAliasChange, onSecretChange, pending, lockedIdentity, providers, }: {
    provider: string;
    alias: string;
    secret?: string;
    onProviderChange: (value: string) => void;
    onAliasChange: (value: string) => void;
    onSecretChange?: (value: string) => void;
    pending?: boolean;
    lockedIdentity?: boolean;
    providers?: readonly {
        id: string;
        label: string;
    }[];
}): React.JSX.Element;
export declare function ApiKeyForm({ onSave, accounts, }: {
    onSave: (input: ApiKeyEnrollment) => Promise<void>;
    accounts?: readonly {
        provider: string;
        alias: string;
    }[];
}): React.JSX.Element;
