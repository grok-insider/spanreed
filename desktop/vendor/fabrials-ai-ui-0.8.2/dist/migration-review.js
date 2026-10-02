"use client";
import { jsxs, jsx } from "react/jsx-runtime";
function MigrationReviewDetails({
  review
}) {
  return /* @__PURE__ */ jsxs("section", { "aria-label": "Migration review", className: "fb-form", children: [
    /* @__PURE__ */ jsxs("p", { children: [
      /* @__PURE__ */ jsx("strong", { children: "From" }),
      " ",
      /* @__PURE__ */ jsx("code", { children: review.sourceEnvironment }),
      /* @__PURE__ */ jsx("br", {}),
      /* @__PURE__ */ jsx("strong", { children: "To" }),
      " ",
      /* @__PURE__ */ jsx("code", { children: review.destinationEnvironment })
    ] }),
    /* @__PURE__ */ jsx("ul", { children: review.items.map((item) => /* @__PURE__ */ jsxs("li", { style: { overflowWrap: "anywhere" }, children: [
      /* @__PURE__ */ jsx("strong", { children: item.sourceId }),
      " →",
      " ",
      /* @__PURE__ */ jsxs("strong", { children: [
        item.provider,
        "/",
        item.targetAlias
      ] }),
      /* @__PURE__ */ jsx("p", { className: "fb-muted", children: item.action === "copyApiKey" ? "Copy API key. The imported account starts inactive." : "Authorize a new OAuth connection in the destination. Existing tokens are never copied." })
    ] }, item.sourceId)) }),
    /* @__PURE__ */ jsx("p", { className: "fb-muted", children: "Source accounts remain available. Imported accounts do not change your active account or routing." })
  ] });
}
export {
  MigrationReviewDetails
};
