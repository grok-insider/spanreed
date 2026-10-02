import { type ComponentProps, type ReactNode } from "react";
import { Button } from "./controls";
/** `"mod"` prints the main modifier of this device ("Ctrl", or "⌘" on Apple after hydration); anything else is a key cap. */
export type CommandTriggerKey = "mod" | (string & {});
/**
 * The launcher of a command palette: a button that looks like the field it opens, with the shortcut beside its label. It is
 * icon-only when it sits in a container named `command` narrower than 12rem (the command slot of `AppHeader`), or with
 * `compact`; the label then stays as the accessible name. The key hint hides on coarse pointers, where nobody has a
 * keyboard shortcut to learn. `keys` are key caps, `"mod"` being the main modifier resolved after hydration, so the
 * server markup is exact; `aria-keyshortcuts` is derived from them (`Control+K Meta+K`) and stays constant.
 */
export declare function CommandTrigger({ icon, label, name, keys, sequence, separator, shortcut, compact, variant, className, ...props }: Omit<ComponentProps<typeof Button>, "size" | "children" | "aria-label"> & {
    /** Defaults to a magnifier. Decorative: it is hidden from assistive technology. */
    icon?: ReactNode;
    /** The visible text ("Search or run a command"). */
    label: ReactNode;
    /** The accessible name when it must say more than the visible label; it should contain it (WCAG 2.5.3). Defaults to `label` when that is a string. */
    name?: string;
    keys?: readonly CommandTriggerKey[];
    /** The keys are pressed one after the other ("g then k"), not together. `aria-keyshortcuts` cannot express steps, so none is derived. */
    sequence?: boolean;
    /** The word between the keys of a sequence (copy a product translates). */
    separator?: ReactNode;
    /** `aria-keyshortcuts`; `false` omits it (the shortcut is switched off). Derived from `keys` by default. */
    shortcut?: string | false;
    /** Icon only, whatever the container's width. */
    compact?: boolean;
    /** The button's look (`secondary`, a grey fill, by default). `outline` is the card fill and hairline of a launcher that looks like a field. */
    variant?: ComponentProps<typeof Button>["variant"];
}): import("react").JSX.Element;
/**
 * A listbox for a palette that is not cmdk (a list your product filters and moves through with `aria-activedescendant`).
 * It is a size container named `fui-command-options`, so a row can lay itself out by the list's width. Give it an `aria-label`.
 */
export declare function CommandOptionList({ className, ...props }: ComponentProps<"ul">): import("react").JSX.Element;
/**
 * A row of that listbox, with the look of `CommandItem`. `active` is the virtual focus (the row `aria-activedescendant`
 * names): the accent fill and the 2 px Stormlight bar. `disabled` sets `aria-disabled` and mutes the label but keeps the row
 * selectable, so activating it can say why (`reason`); it is not `pointer-events: none`. Slots: `icon`, `label`, `detail`
 * (after the label), `group` (the category, at the end of the line), `reason` and `keys` (`Kbd`s, at the inline end).
 * Below 32rem of list width the label takes its own line, then detail and group, then the reason.
 */
export declare function CommandOption({ icon, label, detail, group, reason, keys, active, disabled, className, ...props }: Omit<ComponentProps<"li">, "children"> & {
    icon?: ReactNode;
    label: ReactNode;
    detail?: ReactNode;
    group?: ReactNode;
    reason?: ReactNode;
    keys?: ReactNode;
    active?: boolean;
    disabled?: boolean;
}): import("react").JSX.Element;
