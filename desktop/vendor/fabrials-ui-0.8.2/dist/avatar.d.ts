import { Avatar as BaseAvatar } from "@base-ui/react/avatar";
import { type StyledProps } from "./shared";
export declare function Avatar({ className, size, ...props }: StyledProps<BaseAvatar.Root.Props> & {
    size?: "sm" | "md" | "lg";
}): import("react").JSX.Element;
export declare function AvatarImage({ className, ...props }: StyledProps<BaseAvatar.Image.Props>): import("react").JSX.Element;
export declare function AvatarFallback({ className, ...props }: StyledProps<BaseAvatar.Fallback.Props>): import("react").JSX.Element;
/**
 * The text of an avatar fallback. A name gives the first letters of its first and last word; an address (no space) gives
 * the first letter of its local part. Grapheme-safe (an emoji or an accented letter is one) and upper-cased in `locale`;
 * `fallback` when there is nothing to show.
 */
export declare function avatarInitials(value: string, fallback?: string, locale?: string): string;
