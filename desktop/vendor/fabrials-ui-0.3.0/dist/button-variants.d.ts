export type ButtonStyleProps = {
    variant?: "default" | "outline" | "secondary" | "ghost" | "destructive" | "link";
    size?: "default" | "sm" | "lg" | "icon" | "icon-sm";
    className?: string;
};
export declare function buttonVariants({ variant, size, className }?: ButtonStyleProps): string;
