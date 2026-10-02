import { type ComponentProps, type ReactNode } from "react";
import { type ButtonProps } from "./controls";
import { AlertDialogContent } from "./dialog";
export type ConfirmResult = {
    ok: true;
} | {
    ok: false;
    error: string;
};
type PopupFinalFocus = NonNullable<ComponentProps<typeof AlertDialogContent>["finalFocus"]>;
type PopupCloseType = PopupFinalFocus extends infer F ? (F extends (closeType: infer C) => unknown ? C : never) : never;
/** Whether the dialog closed because the action ran and succeeded, or was dismissed (Cancel, Escape, the backdrop). */
export type ConfirmOutcome = "confirmed" | "dismissed";
/**
 * Where focus goes when the dialog closes: Base UI's `finalFocus` (`false` for nowhere, a ref, or a function returning an
 * element), with the outcome as a second argument. A confirmed action that removes or disables its trigger names the
 * element that now holds the result; Cancel and Escape still return to the trigger. A function that returns nothing or
 * `null` keeps the default (Base UI treats a bare `undefined` as "do not move focus"; this wrapper does not).
 */
export type ConfirmFinalFocus = boolean | {
    current: HTMLElement | null;
} | ((closeType: PopupCloseType, outcome: ConfirmOutcome) => boolean | HTMLElement | null | void);
export type ConfirmDialogProps = {
    open: boolean;
    onOpenChange: (open: boolean) => void;
    title: ReactNode;
    description?: ReactNode;
    details?: ReactNode;
    confirmLabel: ReactNode;
    pendingLabel?: ReactNode;
    cancelLabel?: ReactNode;
    destructive?: boolean;
    fallbackError?: string;
    onConfirm: () => Promise<ConfirmResult | void> | ConfirmResult | void;
    /** Where focus lands on close. Without it Base UI returns to the trigger, which is nowhere once a confirmed action removes it. */
    finalFocus?: ConfirmFinalFocus;
    className?: string;
};
export declare function ConfirmDialog({ open, onOpenChange, title, description, details, confirmLabel, pendingLabel, cancelLabel, destructive, fallbackError, onConfirm, finalFocus, className, }: ConfirmDialogProps): import("react").JSX.Element;
export type ConfirmActionButtonProps = Omit<ConfirmDialogProps, "open" | "onOpenChange"> & {
    children: ReactNode;
    buttonProps?: Omit<ButtonProps, "onClick" | "children">;
    defaultOpen?: boolean;
};
export declare function ConfirmActionButton({ children, buttonProps, defaultOpen, ...dialog }: ConfirmActionButtonProps): import("react").JSX.Element;
export {};
