"use client";
import { jsxs, jsx, Fragment } from "react/jsx-runtime";
import { Button, Card, CardHeader, CardContent } from "@fabrials/ui";
import * as React from "react";
import { ProviderCard } from "./providers.js";
function PrivateHistoryView({
  fetchPage,
  description
}) {
  const [pages, setPages] = React.useState([]);
  const [busy, setBusy] = React.useState(true);
  const [error, setError] = React.useState(null);
  const generation = React.useRef(0);
  const load = React.useCallback(
    async (before = null) => {
      const current = ++generation.current;
      try {
        const page = await fetchPage(before);
        if (current === generation.current) {
          setError(null);
          setPages(
            (previous) => before === null ? [page] : [...previous, page]
          );
        }
      } catch (error2) {
        if (current === generation.current) setError(String(error2));
      } finally {
        if (current === generation.current) setBusy(false);
      }
    },
    [fetchPage]
  );
  const invalidate = React.useCallback(() => {
    generation.current++;
  }, []);
  React.useEffect(() => {
    const current = ++generation.current;
    fetchPage(null).then((page) => {
      if (current === generation.current) setPages([page]);
    }).catch((error2) => {
      if (current === generation.current) setError(String(error2));
    }).finally(() => {
      if (current === generation.current) setBusy(false);
    });
    return invalidate;
  }, [fetchPage, invalidate]);
  const last = pages.at(-1);
  return /* @__PURE__ */ jsxs("section", { className: "fb-form", children: [
    /* @__PURE__ */ jsxs("div", { className: "fb-heading", children: [
      /* @__PURE__ */ jsx("h2", { children: "Private synchronized history" }),
      /* @__PURE__ */ jsxs("p", { className: "fb-muted", children: [
        description,
        " These observations are separate from local request totals."
      ] })
    ] }),
    /* @__PURE__ */ jsx(
      Button,
      {
        variant: "outline",
        className: "",
        disabled: busy,
        onClick: () => {
          setBusy(true);
          setError(null);
          void load();
        },
        children: "Refresh private history"
      }
    ),
    error && /* @__PURE__ */ jsx("p", { role: "alert", className: "fb-error", children: error }),
    !busy && !error && !pages.some((page) => page.observations.length) && /* @__PURE__ */ jsx("p", { children: "No synchronized observations yet." }),
    /* @__PURE__ */ jsx("div", { className: "fb-grid", children: pages.flatMap((page) => page.observations).map((item) => /* @__PURE__ */ jsx(Observation, { item }, item.cursor)) }),
    last?.has_more && /* @__PURE__ */ jsx(
      Button,
      {
        variant: "outline",
        className: "",
        disabled: busy,
        onClick: () => {
          setBusy(true);
          setError(null);
          void load(last.before);
        },
        children: "Load older observations"
      }
    ),
    busy && /* @__PURE__ */ jsx("p", { role: "status", children: "Loading private history…" })
  ] });
}
function Observation({ item }) {
  const { event, source } = item.observation;
  const at = event.kind === "quota" ? event.at_ms : event.kind === "history" ? event.sample.ts_ms : event.record.ts_ms;
  return /* @__PURE__ */ jsxs(Card, { className: "fb-card-layout", children: [
    /* @__PURE__ */ jsxs(CardHeader, { children: [
      /* @__PURE__ */ jsx("h3", { children: source }),
      /* @__PURE__ */ jsx("time", { children: new Date(at).toLocaleString() }),
      /* @__PURE__ */ jsxs("p", { className: "fb-muted", children: [
        "Installation ",
        item.device
      ] })
    ] }),
    /* @__PURE__ */ jsx(CardContent, { className: "fb-content-layout", children: event.kind === "quota" ? /* @__PURE__ */ jsx(ProviderCard, { provider: event.output }) : event.kind === "history" ? /* @__PURE__ */ jsxs(Fragment, { children: [
      /* @__PURE__ */ jsxs("p", { children: [
        event.sample.label,
        ": ",
        event.sample.used,
        " / ",
        event.sample.limit
      ] }),
      event.sample.resets_at && /* @__PURE__ */ jsxs("p", { children: [
        "Resets ",
        new Date(event.sample.resets_at).toLocaleString()
      ] })
    ] }) : /* @__PURE__ */ jsxs(Fragment, { children: [
      /* @__PURE__ */ jsx("p", { children: event.record.model ?? "Unknown model" }),
      /* @__PURE__ */ jsxs("p", { children: [
        event.record.input_tokens,
        " input · ",
        event.record.output_tokens,
        " ",
        "output tokens"
      ] })
    ] }) })
  ] });
}
export {
  PrivateHistoryView
};
