/**
 * A small syntax highlighter for documentation and product code panels:
 * TypeScript/JavaScript (and JSX), JSON, shell, CSS and HTML. It turns code
 * into lines of classified tokens, pure and server-safe, so CodePanel can
 * render it without shipping a grammar engine. Hosts that already use Shiki
 * can pass their own highlighted children instead.
 */
export type SyntaxToken = {
    text: string;
    type?: SyntaxKind;
};
export type SyntaxKind = "keyword" | "string" | "number" | "function" | "type" | "comment" | "punctuation" | "tag" | "attribute" | "property" | "variable";
/** Languages with rules; anything else renders as plain text. */
export declare const highlightedLanguages: string[];
/** Code as lines of tokens. Tokens that span lines (comments, template strings) are split per line. */
export declare function highlightCode(code: string, language?: string): SyntaxToken[][];
/** The commands each package manager uses to run and to install packages. */
export declare const PACKAGE_MANAGERS: {
    readonly npm: {
        readonly run: "npx";
        readonly install: "npm install";
    };
    readonly pnpm: {
        readonly run: "pnpm dlx";
        readonly install: "pnpm add";
    };
    readonly yarn: {
        readonly run: "yarn dlx";
        readonly install: "yarn add";
    };
    readonly bun: {
        readonly run: "bunx --bun";
        readonly install: "bun add";
    };
};
export type PackageManager = keyof typeof PACKAGE_MANAGERS;
/** "shadcn@latest add @fabrials/button" as each package manager writes it. */
export declare function packageCommand(manager: PackageManager, command: string, kind?: "run" | "install"): string;
