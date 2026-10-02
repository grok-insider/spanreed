import { type ComponentProps, type ReactNode } from "react";
/** A file tree: what a command creates, what a package contains. Folders open and close. */
export declare function Files({ className, children, ...props }: ComponentProps<"ul">): import("react").JSX.Element;
export declare function Folder({ name, defaultOpen, note, children, }: {
    name: string;
    defaultOpen?: boolean;
    /** A short remark after the name, such as "new" or "generated". */
    note?: ReactNode;
    children?: ReactNode;
}): import("react").JSX.Element;
export declare function File({ name, icon, note, highlighted, }: {
    name: string;
    icon?: ReactNode;
    note?: ReactNode;
    /** Marks the file the text is about. */
    highlighted?: boolean;
}): import("react").JSX.Element;
/**
 * A repository as a compact link: owner/name, stars and forks. Presentational:
 * the host fetches the numbers (and caches them); unknown counts are omitted,
 * never shown as zero.
 */
export declare function RepoInfo({ owner, repo, href, stars, forks, description, className, ...props }: Omit<ComponentProps<"a">, "children" | "href"> & {
    owner: string;
    repo: string;
    href?: string;
    stars?: number | null;
    forks?: number | null;
    description?: ReactNode;
}): import("react").JSX.Element;
