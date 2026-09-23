import * as React from "react";
import { providerIcons } from "./provider-icon-data";
export declare function providerBrand(provider: string): keyof typeof providerIcons | null;
export declare function ProviderIcon({ provider, size, className, }: {
    provider: string;
    size?: number;
    className?: string;
}): React.JSX.Element;
