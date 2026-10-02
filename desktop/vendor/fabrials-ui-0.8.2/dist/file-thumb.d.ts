import { type ReactNode } from "react";
export declare function fileTypeLabel(filename?: string | null, mime?: string | null): string;
export type FileThumbProps = {
    src?: string | null;
    mime?: string | null;
    filename?: string | null;
    alt?: string;
    icon?: ReactNode;
    size?: number;
    className?: string;
};
export declare function FileThumb({ src, mime, filename, alt, icon, size, className, }: FileThumbProps): import("react").JSX.Element;
