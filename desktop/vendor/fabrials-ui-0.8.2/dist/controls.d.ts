import { type ComponentProps, type ReactNode } from "react";
import { Button as BaseButton } from "@base-ui/react/button";
import { Checkbox as BaseCheckbox } from "@base-ui/react/checkbox";
import { Switch as BaseSwitch } from "@base-ui/react/switch";
import { type StyledProps } from "./shared";
export { buttonVariants } from "./button-variants";
import type { ButtonStyleProps } from "./button-variants";
export type ButtonProps = StyledProps<BaseButton.Props> & ButtonStyleProps & {
    /** Keeps the label, adds a spinner and blocks repeated activation. */
    loading?: boolean;
};
export declare function Button({ className, variant, size, loading, disabled, focusableWhenDisabled, children, ...props }: ButtonProps): import("react").JSX.Element;
export declare function Input({ className, ...props }: ComponentProps<"input">): import("react").JSX.Element;
export declare function Textarea({ className, ...props }: ComponentProps<"textarea">): import("react").JSX.Element;
export declare function NativeSelect({ className, ...props }: ComponentProps<"select">): import("react").JSX.Element;
/** shadcn's names for the native select's options. */
export declare function NativeSelectOption(props: ComponentProps<"option">): import("react").JSX.Element;
export declare function NativeSelectOptGroup(props: ComponentProps<"optgroup">): import("react").JSX.Element;
type NativeChoiceProps = Omit<ComponentProps<"input">, "type"> & {
    /** Puts the input in a whole-row label, a 44 px target; the text is the input's name. */
    label?: ReactNode;
    labelClassName?: string;
};
export type NativeCheckboxProps = NativeChoiceProps & {
    type?: "checkbox";
    /** Mixed state (some of a group are ticked). It is a property of the element, never an attribute, so
     * it is applied after mount and again after every render and change: a click clears it in the browser
     * even when the host's state does not change. */
    indeterminate?: boolean;
};
export declare function NativeCheckbox({ indeterminate, onChange, ref, ...props }: NativeCheckboxProps): import("react").JSX.Element;
/** The browser's own radio: submits with the form, keeps the arrow keys of its name group and is disabled by a
 * disabled fieldset (a Base UI radio is a span that a fieldset does not disable). Use `NativeRadioGroup`. */
export declare function NativeRadio(props: NativeChoiceProps): import("react").JSX.Element;
export declare function Label({ className, ...props }: ComponentProps<"label">): import("react").JSX.Element;
export declare function Checkbox({ className, indeterminate, ...props }: StyledProps<BaseCheckbox.Root.Props>): import("react").JSX.Element;
export declare function Switch({ className, ...props }: StyledProps<BaseSwitch.Root.Props>): import("react").JSX.Element;
