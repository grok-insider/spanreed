import { type ReactNode } from "react";
import { type ConfirmFinalFocus } from "./confirm-dialog";
export type ConfirmOptions = {
    title: ReactNode;
    description?: ReactNode;
    /** Optional list of the affected records, above the buttons. */
    details?: ReactNode;
    confirmLabel?: ReactNode;
    cancelLabel?: ReactNode;
    /** The confirm button uses the destructive variant. */
    destructive?: boolean;
    /**
     * Plain text for `window.confirm` (no provider, or a title that is not a string). When it is left out the string
     * parts of `title` and `description` are joined; a node with no text falls back to "Are you sure?".
     */
    fallbackText?: string;
    /** Where focus lands when the dialog closes. By default: the element that was focused when `confirm` was called. */
    finalFocus?: ConfirmFinalFocus;
};
export type ConfirmFunction = (options: ConfirmOptions) => Promise<boolean>;
/**
 * Mount once near the root. `useConfirm()` below then opens a `ConfirmDialog` and resolves the promise: `true` when the
 * person confirms, `false` for Cancel, Escape, the backdrop or an unmounted provider. A second question asked while one
 * is open waits its turn and is shown in a fresh dialog (its own focus, starting on Cancel, and its own outcome), so a
 * stray second press cannot confirm it. Focus returns to the element that was focused when `confirm` was called (a control that is gone
 * falls back to Base UI's default).
 */
export declare function ConfirmProvider({ children }: {
    children?: ReactNode;
}): import("react").JSX.Element;
/**
 * `const confirm = useConfirm(); if (await confirm({ title: "Delete draft?", destructive: true })) remove();`
 * The function keeps its identity. Without a `ConfirmProvider` it is `window.confirm` (so a library can call it
 * unconditionally). Not for synchronous guards (back, forward, unload): those cannot wait for a dialog.
 */
export declare function useConfirm(): ConfirmFunction;
