import type { ComponentProps, ReactNode } from "react";
/**
 * A group of related fields: a real `fieldset`, so a screen reader announces the legend and `disabled`
 * disables every native control inside it (Base UI checkboxes, switches and radios are spans and are not
 * covered: disable those yourself). It never overflows its column: `min-inline-size` is 0, where a bare
 * fieldset sizes to its widest child.
 */
export declare function FieldSet({ className, ...props }: ComponentProps<"fieldset">): import("react").JSX.Element;
/**
 * The name of a `FieldSet`. `variant="title"` is a subsection title (16 px, semibold), `"label"` a small group
 * name (14 px, medium). The hairline above (`divider`, on by default) is drawn on the legend itself, because a
 * fieldset's own border would run through the legend's text, and it is left out when the fieldset is the first
 * child of its container. Hide a legend the design does not show with `className="fui-sr-only"`, never `hidden`.
 */
export declare function FieldLegend({ className, variant, divider, ...props }: ComponentProps<"legend"> & {
    variant?: "title" | "label";
    divider?: boolean;
}): import("react").JSX.Element;
/**
 * Fields laid out with the house gap. `layout="columns"` flows them into as many columns of at least 15rem as
 * the container has room for (a container, not the window: it works in a narrow pane and a wide dialog alike).
 */
export declare function FieldGroup({ className, layout, ...props }: ComponentProps<"div"> & {
    layout?: "stack" | "columns";
}): import("react").JSX.Element;
/**
 * A group of native radios sharing one `name`: a fieldset with its legend, mirroring `NativeSelect` and
 * `NativeCheckbox`. A disabled fieldset disables every radio. The arrow keys move within the group natively.
 * `layout="grid"` flows the options into columns of at least `--fui-native-radio-min` (9rem unless you set it on the group,
 * radio and gap included) and never wider than the group, so a narrow pane gets fewer columns instead of crushed labels.
 * Long names: `style={{ "--fui-native-radio-min": "12rem" }}` (swatch and colour pickers).
 */
export declare function NativeRadioGroup({ legend, hideLegend, layout, className, children, ...props }: Omit<ComponentProps<"fieldset">, "children"> & {
    legend: ReactNode;
    hideLegend?: boolean;
    layout?: "stack" | "grid";
    children: ReactNode;
}): import("react").JSX.Element;
