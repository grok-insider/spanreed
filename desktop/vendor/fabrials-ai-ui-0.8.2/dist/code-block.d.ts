import { type HTMLAttributes, type ReactNode } from "react";
export declare function codeFilename(language: string | undefined, base?: string): string;
export declare function useCopyToClipboard(resetMs?: number): {
    copied: boolean;
    copy: (text: string, write?: (text: string) => void | Promise<void>) => Promise<boolean>;
};
export type CodeBlockProps = Omit<HTMLAttributes<HTMLDivElement>, "children" | "onCopy"> & {
    code: string;
    language?: string;
    children?: ReactNode;
    lineNumbers?: boolean;
    onCopy?: (code: string) => void | Promise<void>;
    copyable?: boolean;
    download?: boolean | ((code: string) => void);
    filename?: string;
    copyLabel?: string;
    copiedLabel?: string;
    downloadLabel?: string;
    actions?: ReactNode;
};
export declare function CodeBlock({ code, language, children, lineNumbers, onCopy, copyable, download, filename, copyLabel, copiedLabel, downloadLabel, actions, className, ...props }: CodeBlockProps): import("react").JSX.Element;
