"use client";
import { jsxs, jsx } from "react/jsx-runtime";
import { useState, useRef, useEffect, useCallback } from "react";
import { DownloadIcon, CheckIcon, CopyIcon } from "./chat-icons.js";
const EXTENSIONS = {
  bash: "sh",
  c: "c",
  cpp: "cpp",
  css: "css",
  go: "go",
  html: "html",
  java: "java",
  javascript: "js",
  js: "js",
  json: "json",
  jsx: "jsx",
  kotlin: "kt",
  markdown: "md",
  md: "md",
  nix: "nix",
  python: "py",
  py: "py",
  ruby: "rb",
  rust: "rs",
  rs: "rs",
  sh: "sh",
  shell: "sh",
  sql: "sql",
  swift: "swift",
  toml: "toml",
  ts: "ts",
  tsx: "tsx",
  typescript: "ts",
  yaml: "yaml",
  yml: "yml",
  zig: "zig",
  zsh: "sh"
};
function codeFilename(language, base = "snippet") {
  const key = (language ?? "").trim().toLowerCase();
  return `${base}.${EXTENSIONS[key] ?? "txt"}`;
}
function useCopyToClipboard(resetMs = 2e3) {
  const [copied, setCopied] = useState(false);
  const timer = useRef(void 0);
  useEffect(() => () => clearTimeout(timer.current), []);
  const copy = useCallback(
    async (text, write) => {
      if (write) await write(text);
      else if (typeof navigator !== "undefined" && navigator.clipboard) {
        await navigator.clipboard.writeText(text);
      } else return false;
      setCopied(true);
      clearTimeout(timer.current);
      timer.current = setTimeout(() => setCopied(false), resetMs);
      return true;
    },
    [resetMs]
  );
  return { copied, copy };
}
function downloadText(text, filename, mediaType = "text/plain") {
  if (typeof document === "undefined") return;
  const url = URL.createObjectURL(new Blob([text], { type: mediaType }));
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = filename;
  anchor.click();
  setTimeout(() => URL.revokeObjectURL(url), 0);
}
function CodeBlock({
  code,
  language,
  children,
  lineNumbers = false,
  onCopy,
  copyable = true,
  download = false,
  filename,
  copyLabel = "Copy code",
  copiedLabel = "Copied",
  downloadLabel = "Download file",
  actions,
  className,
  ...props
}) {
  const { copied, copy } = useCopyToClipboard();
  const label = language?.trim() || "text";
  const lines = code.replace(/\n$/, "").split("\n");
  return /* @__PURE__ */ jsxs(
    "div",
    {
      className: ["fui-code-block", className].filter(Boolean).join(" "),
      "data-language": label,
      "data-line-numbers": lineNumbers || void 0,
      ...props,
      children: [
        /* @__PURE__ */ jsxs("div", { className: "fui-code-block-header", children: [
          /* @__PURE__ */ jsx("span", { className: "fui-code-block-language", children: label }),
          /* @__PURE__ */ jsxs("div", { className: "fui-code-block-actions", children: [
            actions,
            download ? /* @__PURE__ */ jsx(
              "button",
              {
                "aria-label": downloadLabel,
                className: "fui-code-block-action",
                onClick: () => typeof download === "function" ? download(code) : downloadText(code, filename ?? codeFilename(language)),
                title: downloadLabel,
                type: "button",
                children: /* @__PURE__ */ jsx(DownloadIcon, {})
              }
            ) : null,
            copyable ? /* @__PURE__ */ jsx(
              "button",
              {
                "aria-label": copied ? copiedLabel : copyLabel,
                className: "fui-code-block-action",
                "data-copied": copied || void 0,
                onClick: () => void copy(code, onCopy).catch(() => void 0),
                title: copied ? copiedLabel : copyLabel,
                type: "button",
                children: copied ? /* @__PURE__ */ jsx(CheckIcon, {}) : /* @__PURE__ */ jsx(CopyIcon, {})
              }
            ) : null
          ] })
        ] }),
        /* @__PURE__ */ jsx("div", { className: "fui-code-block-body", role: "region", "aria-label": `${label} code`, tabIndex: 0, children: /* @__PURE__ */ jsx("pre", { children: /* @__PURE__ */ jsx("code", { children: children ?? lines.map((line, index) => /* @__PURE__ */ jsx("span", { className: "fui-code-line", children: line }, index)) }) }) })
      ]
    }
  );
}
export {
  CodeBlock,
  codeFilename,
  useCopyToClipboard
};
