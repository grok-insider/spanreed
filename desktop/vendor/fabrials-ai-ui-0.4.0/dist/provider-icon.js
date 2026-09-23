"use client";
import { jsx, jsxs } from "react/jsx-runtime";
import { providerIcons } from "./provider-icon-data.js";
function providerBrand(provider) {
  const name = provider.split("/")[0].toLowerCase();
  const aliases = {
    codex: "openai",
    openai: "openai",
    claude: "anthropic",
    anthropic: "anthropic",
    cursor: "cursor",
    grok: "grok",
    xai: "grok",
    nous: "nousresearch",
    opencode: "opencode",
    "opencode-go": "opencode"
  };
  return aliases[name] ?? null;
}
function ProviderIcon({
  provider,
  size = 24,
  className = ""
}) {
  const brand = providerBrand(provider);
  return /* @__PURE__ */ jsx(
    "span",
    {
      "aria-hidden": "true",
      className: `fb-provider-icon ${className}`,
      style: {
        width: size,
        height: size,
        display: "inline-flex",
        flexShrink: 0
      },
      children: brand ? /* @__PURE__ */ jsx(
        "span",
        {
          style: { display: "contents" },
          dangerouslySetInnerHTML: { __html: providerIcons[brand] }
        }
      ) : /* @__PURE__ */ jsxs(
        "svg",
        {
          "data-icon": "fallback",
          viewBox: "0 0 24 24",
          fill: "none",
          stroke: "currentColor",
          strokeWidth: "1.5",
          children: [
            /* @__PURE__ */ jsx("rect", { x: "4", y: "4", width: "16", height: "16", rx: "4" }),
            /* @__PURE__ */ jsx("path", { d: "M8 9h8M8 12h8M8 15h5" })
          ]
        }
      )
    }
  );
}
export {
  ProviderIcon,
  providerBrand
};
