import { Radio as BaseRadio } from "@base-ui/react/radio";
import { RadioGroup as BaseRadioGroup } from "@base-ui/react/radio-group";
import { type StyledProps } from "./shared";
export declare function RadioGroup({ className, ...props }: StyledProps<BaseRadioGroup.Props>): import("react").JSX.Element;
/** shadcn's name for Radio. */
export declare const RadioGroupItem: typeof Radio;
export declare function Radio({ className, ...props }: StyledProps<BaseRadio.Root.Props>): import("react").JSX.Element;
