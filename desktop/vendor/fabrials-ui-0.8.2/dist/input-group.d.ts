import type * as React from "react";
import { Button } from "./controls";
declare function InputGroup({ className, ...props }: React.ComponentProps<"div">): React.JSX.Element;
type AddonAlign = "inline-start" | "inline-end" | "block-start" | "block-end";
declare function InputGroupAddon({ className, align, ...props }: React.ComponentProps<"div"> & {
    align?: AddonAlign | null;
}): React.JSX.Element;
declare function InputGroupButton({ className, type, variant, size, ...props }: Omit<React.ComponentProps<typeof Button>, "size" | "type"> & {
    size?: "xs" | "sm" | "icon-xs" | "icon-sm" | null;
    type?: "button" | "submit" | "reset";
}): React.JSX.Element;
declare function InputGroupText({ className, ...props }: React.ComponentProps<"span">): React.JSX.Element;
declare function InputGroupInput({ className, ...props }: React.ComponentProps<"input">): React.JSX.Element;
declare function InputGroupTextarea({ className, ...props }: React.ComponentProps<"textarea">): React.JSX.Element;
export { InputGroup, InputGroupAddon, InputGroupButton, InputGroupText, InputGroupInput, InputGroupTextarea, };
