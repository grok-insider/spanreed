import { Select as BaseSelect } from "@base-ui/react/select";
import { type StyledProps } from "./shared";
export declare const Select: typeof BaseSelect.Root;
export declare const SelectValue: import("react").ForwardRefExoticComponent<Omit<import("@base-ui/react").SelectValueProps, "ref"> & import("react").RefAttributes<HTMLSpanElement>>;
export declare const SelectGroup: import("react").ForwardRefExoticComponent<Omit<import("@base-ui/react").SelectGroupProps, "ref"> & import("react").RefAttributes<HTMLDivElement>>;
export declare function SelectLabel({ className, ...props }: StyledProps<BaseSelect.GroupLabel.Props>): import("react").JSX.Element;
export declare function SelectSeparator({ className, ...props }: StyledProps<BaseSelect.Separator.Props>): import("react").JSX.Element;
export declare function SelectScrollUpButton({ className, ...props }: StyledProps<BaseSelect.ScrollUpArrow.Props>): import("react").JSX.Element;
export declare function SelectScrollDownButton({ className, ...props }: StyledProps<BaseSelect.ScrollDownArrow.Props>): import("react").JSX.Element;
export declare function SelectTrigger({ className, children, size, ...props }: StyledProps<BaseSelect.Trigger.Props> & {
    size?: string;
}): import("react").JSX.Element;
export declare function SelectContent({ className, children, align, ...props }: StyledProps<BaseSelect.Popup.Props> & {
    align?: "start" | "center" | "end";
}): import("react").JSX.Element;
export declare function SelectItem({ className, children, ...props }: StyledProps<BaseSelect.Item.Props>): import("react").JSX.Element;
