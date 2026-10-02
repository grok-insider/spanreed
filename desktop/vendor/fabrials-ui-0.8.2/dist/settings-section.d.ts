import { type ComponentProps, type ReactNode, type Ref } from "react";
import type { HeadingLevel, HeadingProps } from "./patterns";
export type SettingsSectionProps = Omit<ComponentProps<"section">, "title"> & {
    title: ReactNode;
    description?: ReactNode;
    status?: ReactNode;
    /** The heading element (default 2). The look is the section title's at every level. */
    headingLevel?: HeadingLevel;
    /** The heading element, for code that moves focus to it (give it `headingProps={{ tabIndex: -1 }}`). */
    headingRef?: Ref<HTMLHeadingElement>;
    /** Attributes of the heading element: `tabIndex`, `id` (the section is named by whatever id the heading has), `data-*`. */
    headingProps?: HeadingProps;
    /**
     * `auto` (default): help on the left (13rem) and the control on the right from 36rem of the SECTION's own width, one
     * column below it. The section answers to its container, not to the viewport, so the same section is right in a
     * 26rem sheet, a 62rem dialog and a full page. `stacked`: always one column.
     *
     * The section is the size container (inline size), so it takes its width from its parent: keep it in a block, grid
     * or column flex parent, not a content-sized row.
     */
    layout?: "auto" | "stacked";
};
export declare function SettingsSection({ id, title, description, status, headingLevel, headingRef, headingProps, layout, className, children, ...props }: SettingsSectionProps): import("react").JSX.Element;
