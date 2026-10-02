import { type ComponentProps, type ReactNode } from "react";
export type CodePanelProps = Omit<ComponentProps<"figure">, "title" | "children"> & {
    code: string;
    /** ts, tsx, js, json, bash, css, html… Other languages render as plain text. */
    language?: string;
    /** Usually the file name. Without it the bar shows the language. */
    title?: ReactNode;
    icon?: ReactNode;
    lineNumbers?: boolean;
    /** 1-based lines to draw attention to. */
    highlightLines?: number[];
    /** 1-based lines shown as added or removed, for diffs. */
    addedLines?: number[];
    removedLines?: number[];
    /** Words boxed wherever they appear, to point at a name. */
    highlightWords?: string[];
    /** What Copy puts on the clipboard, when it differs from the code (e.g. without removed lines). */
    copyValue?: string;
    /** Pre-highlighted code (from Shiki, for instance) replaces the built-in highlighter. */
    children?: ReactNode;
    /** Hide the bar for inline snippets; the copy button floats instead. */
    bare?: boolean;
};
/**
 * A code sample with its file name, a copy button and optional line numbers,
 * highlighted lines, boxed words and diff marks. Highlights TypeScript,
 * JSON, shell, CSS and HTML itself; pass Shiki output as children to use it.
 */
export declare function CodePanel({ code, language, title, icon, lineNumbers, highlightLines, addedLines, removedLines, highlightWords, copyValue, children, bare, className, ...props }: CodePanelProps): import("react").JSX.Element;
export type CodeTab = Omit<CodePanelProps, "title" | "bare"> & {
    value: string;
    label: ReactNode;
};
/** Several versions of one sample (languages, frameworks, files) in one frame. */
export declare function CodeTabs({ items, defaultValue, value, onValueChange, label, className, }: {
    items: CodeTab[];
    defaultValue?: string;
    value?: string;
    onValueChange?: (value: string) => void;
    label?: string;
    className?: string;
}): import("react").JSX.Element;
/**
 * A command in npm, pnpm, yarn and bun. Picking one switches every
 * PackageInstall on the page; with `persistKey`, the choice is remembered in
 * this browser (read after hydration, never during render).
 */
export declare function PackageInstall({ command, kind, persistKey, className, }: {
    /** Without the runner: "shadcn@latest add @fabrials/button", or "@fabrials/ui" for kind="install". */
    command: string;
    kind?: "run" | "install";
    persistKey?: string | null;
    className?: string;
}): import("react").JSX.Element;
