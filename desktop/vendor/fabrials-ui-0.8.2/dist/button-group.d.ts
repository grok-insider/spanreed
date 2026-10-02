import type * as React from "react";
import { useRender } from "@base-ui/react/use-render";
import { Separator } from "./display";
type ButtonGroupOrientation = "horizontal" | "vertical";
declare function buttonGroupVariants({ orientation, className, }?: {
    orientation?: ButtonGroupOrientation | null;
    className?: string;
}): string;
declare function ButtonGroup({ className, orientation, ...props }: React.ComponentProps<"div"> & {
    orientation?: ButtonGroupOrientation | null;
}): React.JSX.Element;
declare function ButtonGroupText({ className, render, ...props }: useRender.ComponentProps<"div">): React.ReactElement<unknown, string | React.JSXElementConstructor<any>>;
declare function ButtonGroupSeparator({ className, orientation, ...props }: React.ComponentProps<typeof Separator>): React.JSX.Element;
export { ButtonGroup, ButtonGroupSeparator, ButtonGroupText, buttonGroupVariants, };
