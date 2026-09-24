import { Avatar as BaseAvatar } from "@base-ui/react/avatar";
import { type StyledProps } from "./shared";
export declare function Avatar({ className, size, ...props }: StyledProps<BaseAvatar.Root.Props> & {
    size?: "sm" | "md" | "lg";
}): import("react").JSX.Element;
export declare function AvatarImage({ className, ...props }: StyledProps<BaseAvatar.Image.Props>): import("react").JSX.Element;
export declare function AvatarFallback({ className, ...props }: StyledProps<BaseAvatar.Fallback.Props>): import("react").JSX.Element;
