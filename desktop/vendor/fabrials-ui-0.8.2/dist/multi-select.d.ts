import * as React from "react";
export type MultiSelectOption = {
    value: string;
    label: string;
};
export declare function MultiSelect({ id, label, options, value, onValueChange, placeholder, disabled, className, }: {
    id: string;
    label: string;
    options: readonly MultiSelectOption[];
    value: readonly string[];
    onValueChange: (value: string[]) => void;
    placeholder?: string;
    disabled?: boolean;
    className?: string;
}): React.JSX.Element;
