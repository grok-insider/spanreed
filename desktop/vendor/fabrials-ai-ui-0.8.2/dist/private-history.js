"use client";
import { jsxs, jsx } from "react/jsx-runtime";
import { StatePanel, Button } from "@fabrials/ui";
import * as React from "react";
function PrivateHistoryView({
  fetchPage,
  description,
  heading = true
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
          setPages((previous) => before === null ? [page] : [...previous, page]);
        }
      } catch (cause) {
        if (current === generation.current) setError(cause instanceof Error ? cause.message : String(cause));
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
    }).catch((cause) => {
      if (current === generation.current) setError(cause instanceof Error ? cause.message : String(cause));
    }).finally(() => {
      if (current === generation.current) setBusy(false);
    });
    return invalidate;
  }, [fetchPage, invalidate]);
  const observations = pages.flatMap((page) => page.observations);
  const last = pages.at(-1);
  return /* @__PURE__ */ jsxs("section", { className: "fb-history", "aria-label": heading ? void 0 : "Private history", children: [
    heading ? /* @__PURE__ */ jsxs("div", { className: "fb-heading", children: [
      /* @__PURE__ */ jsx("h2", { children: "Private history" }),
      description ? /* @__PURE__ */ jsx("p", { children: description }) : null
    ] }) : description ? /* @__PURE__ */ jsx("p", { className: "fb-muted", children: description }) : null,
    error ? /* @__PURE__ */ jsx("p", { className: "fb-error", role: "alert", children: error }) : null,
    busy && observations.length === 0 ? /* @__PURE__ */ jsx("p", { role: "status", children: "Loading private history…" }) : null,
    !busy && !error && observations.length === 0 ? /* @__PURE__ */ jsx(
      StatePanel,
      {
        description: "Observations appear after a Spanreed installation shares quota or requests with this relay.",
        headingLevel: 3,
        state: "empty",
        title: "No synchronized observations yet"
      }
    ) : null,
    observations.length > 0 ? /* @__PURE__ */ jsx("ol", { className: "fb-log", children: observations.map((item) => /* @__PURE__ */ jsx(Observation, { item }, item.cursor)) }) : null,
    last?.has_more ? /* @__PURE__ */ jsx(
      Button,
      {
        disabled: busy,
        onClick: () => {
          setBusy(true);
          setError(null);
          void load(last.before);
        },
        variant: "outline",
        children: busy ? "Loading older observations…" : "Load older observations"
      }
    ) : null
  ] });
}
function Observation({ item }) {
  const { event, source } = item.observation;
  const at = event.kind === "quota" ? event.at_ms : event.kind === "history" ? event.sample.ts_ms : event.record.ts_ms;
  const title = event.kind === "quota" ? event.output.displayName || source : source;
  const detail = event.kind === "quota" ? quotaDetail(event.output) : event.kind === "history" ? historyDetail(event.sample.label, event.sample.used, event.sample.limit, event.sample.resets_at) : requestDetail(event.record.model, event.record.input_tokens, event.record.output_tokens);
  return /* @__PURE__ */ jsxs("li", { className: "fb-log-item", children: [
    /* @__PURE__ */ jsxs("div", { className: "fb-row", children: [
      /* @__PURE__ */ jsx("strong", { children: title }),
      /* @__PURE__ */ jsx("time", { dateTime: new Date(at).toISOString(), children: formatStamp(at) })
    ] }),
    /* @__PURE__ */ jsx("p", { children: detail }),
    /* @__PURE__ */ jsxs("p", { className: "fb-muted", children: [
      kindLabel(event.kind),
      " · machine ",
      /* @__PURE__ */ jsx("span", { className: "fb-mono", children: shortId(item.device) })
    ] })
  ] });
}
function kindLabel(kind) {
  if (kind === "quota") return "Quota reading";
  if (kind === "history") return "Limit sample";
  return "Request";
}
function quotaDetail(output) {
  const plan = output.plan ? `${output.plan}. ` : "";
  const progress = output.lines.find((line) => line.type === "progress");
  if (progress && progress.type === "progress") {
    if (progress.format.kind === "percent") return `${plan}${progress.label}: ${Math.round(progress.used)}%`;
    const used = formatCount(progress.used);
    const limit = progress.limit > 0 ? formatCount(progress.limit) : null;
    return `${plan}${progress.label}: ${limit ? `${used} of ${limit}` : used}`;
  }
  const text = output.lines.find((line) => line.type === "text");
  if (text && text.type === "text") return `${plan}${text.label}: ${text.value}`;
  const badge = output.lines.find((line) => line.type === "badge");
  if (badge && badge.type === "badge") return `${plan}${badge.label}: ${badge.text}`;
  return plan.trim() ? output.plan ?? "Quota not reported" : "Quota not reported";
}
function historyDetail(label, used, limit, resetsAt) {
  const amount = limit > 0 ? `${formatCount(used)} of ${formatCount(limit)}` : formatCount(used);
  const reset = resetsAt ? Date.parse(resetsAt) : Number.NaN;
  const when = Number.isFinite(reset) ? ` Resets ${formatStamp(reset)}.` : "";
  return `${label}: ${amount}.${when}`;
}
function requestDetail(model, input, output) {
  return `${model || "Unknown model"} · ${formatCount(input)} in · ${formatCount(output)} out`;
}
function formatCount(value) {
  if (!Number.isFinite(value)) return "0";
  const abs = Math.abs(value);
  if (abs >= 1e6) return `${trimNumber(value / 1e6)}M`;
  if (abs >= 1e4) return `${trimNumber(value / 1e3)}k`;
  return new Intl.NumberFormat("en", { maximumFractionDigits: 0 }).format(value);
}
function trimNumber(value) {
  return new Intl.NumberFormat("en", { maximumFractionDigits: 1 }).format(value);
}
function formatStamp(ms) {
  return new Intl.DateTimeFormat("en", { month: "short", day: "numeric", hour: "numeric", minute: "2-digit" }).format(ms);
}
function shortId(value) {
  if (value.length <= 12) return value;
  return `${value.slice(0, 4)}…${value.slice(-4)}`;
}
export {
  PrivateHistoryView
};
