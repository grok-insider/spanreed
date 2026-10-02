import { type ComponentProps, type ReactNode } from "react";
import { Dialog as BaseDialog } from "@base-ui/react/dialog";
import { AlertDialog as BaseAlertDialog } from "@base-ui/react/alert-dialog";
import { Button } from "./controls";
import { type StyledProps } from "./shared";
type ButtonVariant = ComponentProps<typeof Button>["variant"];
/** The dialog root. Escape during an IME composition does not close it (0.8). */
export declare function Dialog<Payload = unknown>({ onOpenChange, ...props }: BaseDialog.Root.Props<Payload>): import("react").JSX.Element;
export declare const DialogTrigger: BaseDialog.Trigger;
export declare const DialogClose: import("react").ForwardRefExoticComponent<Omit<import("@base-ui/react").AlertDialogCloseProps, "ref"> & import("react").RefAttributes<HTMLButtonElement>>;
export declare const DialogPortal: import("react").ForwardRefExoticComponent<Omit<import("@base-ui/react").AlertDialogPortalProps, "ref"> & import("react").RefAttributes<HTMLDivElement>>;
/** The scrim behind a dialog. DialogContent already renders one; use this with DialogPortal for a custom layout. */
export declare function DialogOverlay({ className, ...props }: StyledProps<BaseDialog.Backdrop.Props>): import("react").JSX.Element;
export type DialogContentProps = StyledProps<BaseDialog.Popup.Props> & {
    closeLabel?: string;
    /**
     * The `Button` variant of the Close that `close="footer"` puts in the footer. `"secondary"` (a grey fill) by default; a product
     * whose secondary actions are outlines sets `"outline"` once here. A `DialogFooter` `closeVariant` wins. The corner X is always
     * the ghost icon button.
     */
    closeVariant?: ButtonVariant;
    /**
     * Where the dialog is portalled: an element, a ref to one, or `null` to wait until there is one. The default is the document
     * body. Hold the element in state (`useState` with the setter as its `ref`) when the dialog can be open on the first render: a
     * ref object is still empty then, and the dialog falls back to the body. Put a sheet next to its trigger and the tab order is
     * trigger, then the sheet, with nothing between them. A
     * position-fixed popup is placed against the window whatever the container is, unless the container is itself a containing
     * block (`transform`, `contain: paint`).
     */
    container?: BaseDialog.Portal.Props["container"];
    /** The corner X. `close` overrides it. */
    showCloseButton?: boolean;
    /**
     * Where the close control lives. `"corner"` is the X in the corner (the default); `"footer"` puts a Close button first in
     * the `DialogFooter` (so the primary action ends at the inline end) and draws no X.
     */
    close?: "corner" | "footer";
    placement?: "center" | "start" | "end";
    /**
     * `default` is 32rem. `wide` is 46rem. `settings` is a 62 by 42rem workspace (72 by 48rem from 100rem) for a dialog that holds
     * a rail and panels. `full-narrow` keeps the default width and takes the whole screen below 48rem. `wide` and `settings`
     * also take the whole screen there, the way a sheet does.
     */
    size?: "default" | "wide" | "settings" | "full-narrow";
    /** `none` removes the padding (a sheet whose content draws its own, a dialog that is only a `DialogBody`). */
    padding?: "default" | "none";
    /** Keep the dialog in the DOM, hidden, while it is closed: what was typed in it is still there when it opens again. */
    keepMounted?: boolean;
    /** Draw the scrim. A non-modal Sheet has none. */
    backdrop?: boolean;
};
export declare function DialogContent({ className, children, closeLabel, closeVariant, container, showCloseButton, close, placement, size, padding, keepMounted, backdrop, ...props }: DialogContentProps): import("react").JSX.Element;
export declare function DialogTitle({ className, ...props }: StyledProps<BaseDialog.Title.Props>): import("react").JSX.Element;
export declare function DialogDescription({ className, ...props }: StyledProps<BaseDialog.Description.Props>): import("react").JSX.Element;
export declare function DialogHeader({ className, ...props }: ComponentProps<"div">): import("react").JSX.Element;
/**
 * The scrolling middle of a dialog. Put it between `DialogHeader` and `DialogFooter` as direct children of `DialogContent`
 * and the dialog gets a fixed header, a body that is the only scroller and a fixed footer: a long title, a long form or a
 * long list never pushes the title or the actions out of reach. Without a `DialogBody` a dialog is the plain padded box.
 * `scroll={false}` makes the body a plain region that its own content fills and scrolls (a settings rail and panel).
 * A body with no focusable content should be given `tabIndex={0}` and a name, so it scrolls from the keyboard.
 */
export declare function DialogBody({ className, scroll, padding, ...props }: ComponentProps<"div"> & {
    scroll?: boolean;
    padding?: "default" | "none";
}): import("react").JSX.Element;
/**
 * The action row of a dialog. In a `DialogContent` with `close="footer"` (or with `showCloseButton`, shadcn's name) a Close
 * button comes first, so the primary action is the last one, at the inline end. In a fixed-layout dialog under 34rem the
 * row wraps: the secondary buttons share rows, the ink action (last, as in the tab order) takes the full bottom row, and every
 * button is 44 px tall.
 */
export declare function DialogFooter({ className, children, showCloseButton, closeLabel, closeVariant, ref, ...props }: ComponentProps<"div"> & {
    showCloseButton?: boolean;
    closeLabel?: string;
    /** The variant of the Close this footer draws. Default: the `DialogContent`'s `closeVariant`, else `"secondary"`. */
    closeVariant?: ButtonVariant;
}): import("react").JSX.Element;
/**
 * Renders its children in the dialog's footer, after Close. For actions that belong to a stateful child under the body (a form
 * that owns the submit): the button sits in the fixed footer while the state stays where it lives. Nothing renders until the
 * footer has mounted, so the server output is empty. Needs a `DialogFooter` in the same `DialogContent`.
 */
export declare function DialogActions({ children }: {
    children: ReactNode;
}): import("react").ReactPortal | null;
/**
 * A sheet is a dialog attached to an edge. `modal` is Base UI's: `true` (default) traps focus, locks the page scroll and draws a
 * scrim; `false` is a drawer beside the page (no scrim, no trap, the page stays usable, an outside press does not close it,
 * Escape and the close control do); `"trap-focus"` traps focus but leaves the page usable. Base UI reads `modal` on the
 * root, which is why it is set here and `SheetContent` follows it.
 *
 * `closeOnEscape` says where Escape is heard. `"anywhere"` (the default) is a dialog's: Escape closes it from wherever focus is in
 * the document. `"focus-inside"` closes it only when focus is inside the sheet, so a drawer that stays open beside the page is not
 * closed by an Escape meant for the editor next to it; Escape from inside still hands focus back to the trigger.
 */
export declare function Sheet<Payload = unknown>({ modal, disablePointerDismissal, closeOnEscape, onOpenChange, ...props }: BaseDialog.Root.Props<Payload> & {
    closeOnEscape?: "anywhere" | "focus-inside";
}): import("react").JSX.Element;
export declare const SheetTrigger: BaseDialog.Trigger;
export declare const SheetClose: import("react").ForwardRefExoticComponent<Omit<import("@base-ui/react").AlertDialogCloseProps, "ref"> & import("react").RefAttributes<HTMLButtonElement>>;
export declare const SheetHeader: typeof DialogHeader;
export declare const SheetTitle: typeof DialogTitle;
export declare const SheetDescription: typeof DialogDescription;
export declare const SheetFooter: typeof DialogFooter;
export declare const SheetBody: typeof DialogBody;
/**
 * The panel of a `Sheet`.
 *
 * `side` is `"left"` or `"right"` (physical: the same edge in every writing direction) or `"start"` or `"end"` (logical: the
 * inline start or end, which is the other edge in a right-to-left page). `container` portals it somewhere other than the body
 * (see `DialogContent`), and `initialFocus` / `finalFocus` are Base UI's: `initialFocus={false}` leaves focus where it was when
 * the sheet opens (a drawer that must not steal it from its trigger).
 *
 * Placement is set with custom properties on the popup, from `style` or `className`, with no `!important`:
 * `--fui-sheet-inset-block-start` and `--fui-sheet-inset-block-end` (a length, default 0: a drawer under a header),
 * `--fui-sheet-z` (default: the overlay level, one above the scrim) and `--fui-sheet-width` (default 26rem; never wider than the
 * window).
 */
export declare function SheetContent({ side, ref, ...props }: Omit<DialogContentProps, "placement" | "size" | "backdrop"> & {
    side?: "left" | "right" | "start" | "end";
}): import("react").JSX.Element;
/** The alert dialog root. Escape during an IME composition does not close it (0.8). */
export declare function AlertDialog<Payload = unknown>({ onOpenChange, ...props }: BaseAlertDialog.Root.Props<Payload>): import("react").JSX.Element;
export declare const AlertDialogTrigger: BaseAlertDialog.Trigger;
export declare const AlertDialogClose: import("react").ForwardRefExoticComponent<Omit<import("@base-ui/react").AlertDialogCloseProps, "ref"> & import("react").RefAttributes<HTMLButtonElement>>;
export declare const AlertDialogPortal: import("react").ForwardRefExoticComponent<Omit<import("@base-ui/react").AlertDialogPortalProps, "ref"> & import("react").RefAttributes<HTMLDivElement>>;
export declare function AlertDialogOverlay({ className, ...props }: StyledProps<BaseAlertDialog.Backdrop.Props>): import("react").JSX.Element;
/** An icon above the alert title; decorative, the title still states the risk. */
export declare function AlertDialogMedia({ className, ...props }: ComponentProps<"div">): import("react").JSX.Element;
export declare const AlertDialogHeader: typeof DialogHeader;
export declare function AlertDialogFooter({ className, ...props }: ComponentProps<"div">): import("react").JSX.Element;
export declare const AlertDialogAction: typeof Button;
export declare function AlertDialogCancel({ variant, size, ...props }: StyledProps<BaseAlertDialog.Close.Props> & Pick<ComponentProps<typeof Button>, "variant" | "size">): import("react").JSX.Element;
export declare function AlertDialogContent({ className, ...props }: StyledProps<BaseAlertDialog.Popup.Props>): import("react").JSX.Element;
export declare function AlertDialogTitle({ className, ...props }: StyledProps<BaseAlertDialog.Title.Props>): import("react").JSX.Element;
export declare function AlertDialogDescription({ className, ...props }: StyledProps<BaseAlertDialog.Description.Props>): import("react").JSX.Element;
export {};
