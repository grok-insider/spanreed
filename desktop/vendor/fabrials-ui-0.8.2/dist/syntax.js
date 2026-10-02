const JS_KEYWORDS = "import|from|export|default|function|return|const|let|var|if|else|for|while|do|switch|case|break|continue|new|class|extends|implements|interface|type|enum|as|async|await|yield|try|catch|finally|throw|typeof|instanceof|in|of|void|delete|this|super|null|undefined|true|false|satisfies|readonly|public|private|protected|static|declare|namespace|keyof";
const jsRules = [
  [/\/\/[^\n]*|\/\*[\s\S]*?\*\//y, "comment"],
  [/`(?:[^`\\]|\\[\s\S])*`|"(?:[^"\\\n]|\\.)*"|'(?:[^'\\\n]|\\.)*'/y, "string"],
  [
    /(<\/?)([A-Za-z][\w.:-]*)/y,
    (m) => [
      { text: m[1], type: "punctuation" },
      { text: m[2], type: /^[A-Z]/.test(m[2]) ? "type" : "tag" }
    ]
  ],
  [new RegExp(`\\b(?:${JS_KEYWORDS})\\b`, "y"), "keyword"],
  [/\b\d[\d_]*(?:\.\d+)?(?:e[+-]?\d+)?\b/iy, "number"],
  [/[A-Za-z_$][\w$]*(?=\s*(?:<[^()]*>)?\()/y, "function"],
  [/[A-Za-z_$][\w-]*(?==[{"'])/y, "attribute"],
  [/\b[A-Z][\w$]*/y, "type"],
  [/[A-Za-z_$][\w$]*/y, "variable"],
  [/[{}()[\];,.:=<>+\-*/%!?&|^~@#]/y, "punctuation"]
];
const jsonRules = [
  [/"(?:[^"\\\n]|\\.)*"(?=\s*:)/y, "property"],
  [/"(?:[^"\\\n]|\\.)*"/y, "string"],
  [/\b(?:true|false|null)\b/y, "keyword"],
  [/-?\b\d+(?:\.\d+)?(?:e[+-]?\d+)?\b/iy, "number"],
  [/[{}[\],:]/y, "punctuation"]
];
const shellRules = [
  // A comment starts a line or follows a space; a # inside a word or URL is not one.
  [/(?<![^\s])#[^\n]*/y, "comment"],
  [/"(?:[^"\\\n]|\\.)*"|'[^'\n]*'/y, "string"],
  [/(^|(?<=[\n|;&]\s*))(?:npx|pnpm|yarn|bunx|bun|npm|node|git|curl|docker|cd|export|sudo|cargo|nix)\b/y, "function"],
  [/(?<=\s)--?[\w-]+/y, "attribute"],
  [/\$[\w{}]+/y, "variable"],
  [/[|;&<>\\]/y, "punctuation"]
];
const cssRules = [
  [/\/\*[\s\S]*?\*\//y, "comment"],
  [/"(?:[^"\\\n]|\\.)*"|'(?:[^'\\\n]|\\.)*'/y, "string"],
  [/@[\w-]+/y, "keyword"],
  [/--[\w-]+/y, "variable"],
  [/[\w-]+(?=\s*:[^:{}]*;)/y, "property"],
  [/#[\da-f]{3,8}\b|\b\d+(?:\.\d+)?(?:px|rem|em|%|vh|vw|ms|s|deg)?\b/iy, "number"],
  [/[\w-]+(?=\()/y, "function"],
  [/[{}();:,>+~*]/y, "punctuation"]
];
const htmlRules = [
  [/<!--[\s\S]*?-->/y, "comment"],
  [
    /(<\/?)([A-Za-z][\w-]*)/y,
    (m) => [
      { text: m[1], type: "punctuation" },
      { text: m[2], type: "tag" }
    ]
  ],
  [/[\w-]+(?==)/y, "attribute"],
  [/"[^"]*"|'[^']*'/y, "string"],
  [/\/?>|=/y, "punctuation"]
];
const LANGUAGES = {
  ts: jsRules,
  tsx: jsRules,
  js: jsRules,
  jsx: jsRules,
  mjs: jsRules,
  typescript: jsRules,
  javascript: jsRules,
  json: jsonRules,
  jsonc: jsonRules,
  bash: shellRules,
  sh: shellRules,
  shell: shellRules,
  zsh: shellRules,
  css: cssRules,
  html: htmlRules,
  xml: htmlRules
};
const highlightedLanguages = Object.keys(LANGUAGES);
function tokenize(code, rules) {
  const tokens = [];
  let plain = "";
  let i = 0;
  const flush = () => {
    if (plain) tokens.push({ text: plain });
    plain = "";
  };
  outer: while (i < code.length) {
    for (const [pattern, kind] of rules) {
      pattern.lastIndex = i;
      const match = pattern.exec(code);
      if (match && match[0].length > 0) {
        flush();
        if (typeof kind === "function") tokens.push(...kind(match));
        else tokens.push({ text: match[0], type: kind });
        i += match[0].length;
        continue outer;
      }
    }
    plain += code[i];
    i += 1;
  }
  flush();
  return tokens;
}
function highlightCode(code, language) {
  const rules = LANGUAGES[(language ?? "").toLowerCase()];
  const tokens = rules ? tokenize(code, rules) : [{ text: code }];
  const lines = [[]];
  for (const token of tokens) {
    const parts = token.text.split("\n");
    parts.forEach((part, index) => {
      if (index > 0) lines.push([]);
      if (part) lines[lines.length - 1].push({ text: part, type: token.type });
    });
  }
  return lines;
}
const PACKAGE_MANAGERS = {
  npm: { run: "npx", install: "npm install" },
  pnpm: { run: "pnpm dlx", install: "pnpm add" },
  yarn: { run: "yarn dlx", install: "yarn add" },
  bun: { run: "bunx --bun", install: "bun add" }
};
function packageCommand(manager, command, kind = "run") {
  return `${PACKAGE_MANAGERS[manager][kind]} ${command}`;
}
export {
  PACKAGE_MANAGERS,
  highlightCode,
  highlightedLanguages,
  packageCommand
};
