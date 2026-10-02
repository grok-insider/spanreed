import type { ReactNode } from "react";
/**
 * The keys of a control's shortcut. A string or an array is pressed together ("⌘", "K"); `{ keys, sequence: true }` is
 * pressed one after the other ("g" then "i"), which `aria-keyshortcuts` cannot express, so none is emitted for it.
 */
export type ControlShortcut = string | readonly string[] | {
    keys: readonly string[];
    sequence?: boolean;
    separator?: ReactNode;
};
/** The `aria-keyshortcuts` value for keys as they are printed on a Kbd: "⌘" is "Meta", "Esc" is "Escape". Undefined for a sequence. */
export declare function keyShortcutsValue(shortcut: ControlShortcut | undefined): string | undefined;
/** The tooltip body of an icon control: its name and, when it has one, the shortcut as flat keys. */
export declare function IconTooltipContent({ label, shortcut }: {
    label: ReactNode;
    shortcut?: ControlShortcut;
}): import("react").JSX.Element;
