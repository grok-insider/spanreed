import { type ComponentProps, type ReactNode } from "react";
export type SuggestionCardProps = Omit<ComponentProps<"button">, "title" | "children"> & {
    title: ReactNode;
    description?: ReactNode;
    icon?: ReactNode;
};
export declare function SuggestionCard({ title, description, icon, className, type, ...props }: SuggestionCardProps): import("react").JSX.Element;
export type SuggestionGridProps = ComponentProps<"ul"> & {
    columns?: 1 | 2 | 3 | 4;
};
export declare function SuggestionGrid({ columns, className, style, children, ...props }: SuggestionGridProps): import("react").JSX.Element;
