import { type ComponentProps } from "react";
import { Group, Panel, Separator, useGroupRef, usePanelRef, type GroupImperativeHandle, type Layout, type PanelImperativeHandle } from "react-resizable-panels";
export { useGroupRef as useResizableGroupRef, usePanelRef as useResizablePanelRef };
export type { GroupImperativeHandle as ResizableGroupHandle, Layout as ResizableLayout, PanelImperativeHandle as ResizablePanelHandle };
export type ResizablePanelGroupProps = Omit<ComponentProps<typeof Group>, "className"> & {
    className?: string;
    /** shadcn's name for `orientation`. */
    direction?: "horizontal" | "vertical";
};
/**
 * A row (or column) of panes with draggable and keyboard-operable handles between them. Panes size in percentages,
 * pixels or rem (`minSize="19rem"`), and each scrolls on its own inside the group.
 *
 * Two lessons built in. The hit target of a handle is bigger than its 1 px line: `resizeTargetMinimumSize` defaults
 * to 10 px for a mouse and 24 px for touch (pass your own to change it). And a double click on a handle resets the
 * layout anywhere in that hit target, not only on the line, without the library calling it a user interaction: a
 * host that saves the layout in `onLayoutChanged` would keep the stale one. This wrapper reports the layout that a
 * double click produced as a user interaction (`meta.isUserInteraction`), whatever element was hit, so saving needs
 * no `event.target` logic.
 */
export declare function ResizablePanelGroup({ className, direction, orientation, resizeTargetMinimumSize, elementRef, onLayoutChanged, ...props }: ResizablePanelGroupProps): import("react").JSX.Element;
/** A pane. Sizes are numbers (percent) or strings with a unit (`"18rem"`). */
export declare const ResizablePanel: typeof Panel;
export type ResizableHandleProps = Omit<ComponentProps<typeof Separator>, "className" | "aria-label"> & {
    className?: string;
    /** The handle's name for assistive technology ("Resize the list"). Give every handle one. */
    label?: string;
    /** shadcn's name: a small grip on the line. Off by default; Highstorm draws only the line. */
    withHandle?: boolean;
};
/**
 * The line between two panes: a 1 px hairline; hover, drag and keyboard focus draw a 3 px Stormlight line (keyed on
 * the library's `data-separator`), and a disabled handle is transparent. Arrow keys resize by the library's fixed
 * step, Home and End jump to the limits, a double click resets. The outline is off because it would draw a 5 px rail
 * around a 1 px separator: the line is the focus indicator (forced colours get an outline back).
 */
export declare function ResizableHandle({ className, label, withHandle, children, ...props }: ResizableHandleProps): import("react").JSX.Element;
