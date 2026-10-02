import type { ComponentProps, ReactNode } from "react";
import { useRender } from "@base-ui/react/use-render";
export type NavTabsProps = ComponentProps<"nav"> & {
    /** Accessible name of the navigation landmark. */
    label: string;
};
/**
 * Tabs that navigate: each tab is a link to its own URL, so the host's
 * router owns state and history. Looks like TabsList variant="underline".
 */
export declare function NavTabs({ label, className, children, ...props }: NavTabsProps): import("react").JSX.Element;
export type NavTabProps = Omit<useRender.ComponentProps<"a">, "className"> & {
    /** This tab's page is the one shown. */
    current?: boolean;
    /** A small count after the label. */
    count?: ReactNode;
    className?: string;
};
/** Pass the host's link element through `render`, e.g. `render={<Link href="…" />}`. */
export declare function NavTab({ current, count, render, className, children, ...props }: NavTabProps): import("react").ReactElement<unknown, string | import("react").JSXElementConstructor<any>>;
