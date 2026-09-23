import type { ComponentProps } from "react";
import { Button as BaseButton } from "@base-ui/react/button";
import { Checkbox as BaseCheckbox } from "@base-ui/react/checkbox";
import { Switch as BaseSwitch } from "@base-ui/react/switch";
import { type StyledProps } from "./shared";
export { buttonVariants } from "./button-variants";
import type { ButtonStyleProps } from "./button-variants";
export type ButtonProps = StyledProps<BaseButton.Props> & ButtonStyleProps;
export declare function Button({ className, variant, size, ...props }: ButtonProps): import("react").JSX.Element;
export declare function Input({ className, ...props }: ComponentProps<"input">): import("react").JSX.Element;
export declare function Textarea({ className, ...props }: ComponentProps<"textarea">): import("react").JSX.Element;
export declare function NativeSelect({ className, ...props }: ComponentProps<"select">): import("react").JSX.Element;
export declare function NativeCheckbox({ className, ...props }: ComponentProps<"input"> & {
    type?: "checkbox";
}): import("react").JSX.Element;
export declare function Label({ className, ...props }: ComponentProps<"label">): import("react").JSX.Element;
export declare function Checkbox({ className, indeterminate, ...props }: StyledProps<BaseCheckbox.Root.Props>): import("react").JSX.Element;
export declare function Switch({ className, ...props }: StyledProps<BaseSwitch.Root.Props>): import("react").JSX.Element;
