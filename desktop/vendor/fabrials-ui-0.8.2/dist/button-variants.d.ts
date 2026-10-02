export type ButtonVariant = "default" | "accent" | "outline" | "secondary" | "ghost" | "destructive" | "link";
export type ButtonSize = "default" | "xs" | "sm" | "lg" | "icon" | "icon-xs" | "icon-sm" | "icon-lg";
export type ButtonStyleProps = {
    variant?: ButtonVariant;
    size?: ButtonSize;
    className?: string;
};
export declare function buttonVariants({ variant, size, className }?: ButtonStyleProps): string;
