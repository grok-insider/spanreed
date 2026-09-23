import { type ComponentProps } from "react";
import { Button } from "./controls";
export type CopyButtonProps = Omit<ComponentProps<typeof Button>, "onClick" | "children" | "value"> & {
    value: string;
    label?: string;
    copiedLabel?: string;
    onCopy?: (value: string) => void;
    onCopyError?: (error: unknown) => void;
};
export declare function CopyButton({ value, label, copiedLabel, onCopy, onCopyError, className, variant, size, ...props }: CopyButtonProps): import("react").JSX.Element;
export declare function Snippet({ children, prompt, copyValue, copyLabel, label, className, ...props }: Omit<ComponentProps<"div">, "children"> & {
    /** One command per line. Lines starting with # are shown as comments. */
    children: string | ReadonlyArray<string>;
    prompt?: string | false;
    copyValue?: string;
    copyLabel?: string;
    label?: string;
}): import("react").JSX.Element;
