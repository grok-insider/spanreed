import { Select as BaseSelect } from "@base-ui/react/select";
import { type StyledProps } from "./shared";
export declare const Select: typeof BaseSelect.Root;
export declare const SelectValue: import("react").ForwardRefExoticComponent<Omit<import("@base-ui/react").SelectValueProps, "ref"> & import("react").RefAttributes<HTMLSpanElement>>;
export declare const SelectGroup: import("react").ForwardRefExoticComponent<Omit<import("@base-ui/react").SelectGroupProps, "ref"> & import("react").RefAttributes<HTMLDivElement>>;
export declare function SelectTrigger({ className, children, ...props }: StyledProps<BaseSelect.Trigger.Props>): import("react").JSX.Element;
export declare function SelectContent({ className, children, ...props }: StyledProps<BaseSelect.Popup.Props>): import("react").JSX.Element;
export declare function SelectItem({ className, children, ...props }: StyledProps<BaseSelect.Item.Props>): import("react").JSX.Element;
