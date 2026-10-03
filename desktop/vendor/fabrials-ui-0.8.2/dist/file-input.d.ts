import { type ComponentProps, type ReactNode } from "react";
import { type ButtonStyleProps } from "./button-variants";
export type FileInputProps = Omit<ComponentProps<"input">, "type" | "children" | "value" | "defaultValue" | "size" | "className" | "style"> & {
    /** The visible text of the button, in the interface's language. The browser's own "Choose File" text is never shown. */
    label: ReactNode;
    /** The chosen files (an empty array when the picker was cancelled after a choice). */
    onFilesChange?: (files: File[]) => void;
    /** The text beside the button. Left out, the component shows the names of the files the person chose; pass a node
     * to say something else (an uploaded name, "No file chosen") and `null` to show nothing. */
    fileName?: ReactNode | null;
    /** Adds a clear button after the name while something is shown. The picker is emptied, so choosing the same
     * file again still fires a change, and focus returns to the picker. */
    onClear?: () => void;
    clearLabel?: string;
    description?: ReactNode;
    error?: ReactNode;
    variant?: ButtonStyleProps["variant"];
    size?: ButtonStyleProps["size"];
    /** The wrapper (button, name, description, error). */
    className?: string;
    /** The button itself. */
    buttonClassName?: string;
};
/**
 * A file picker that is a button: a label styled as a `Button` over a visually hidden real input, so the keyboard,
 * the form and the accessibility tree are the browser's and only the look is ours. Focus draws its ring on the
 * button, the file name and the error are read with the input, and a disabled picker looks and acts disabled.
 */
export declare function FileInput({ label, onFilesChange, fileName, onClear, clearLabel, description, error, variant, size, className, buttonClassName, onChange, id, disabled, ref, "aria-describedby": describedBy, ...input }: FileInputProps): import("react").JSX.Element;
