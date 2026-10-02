"use client";
import { jsx, jsxs } from "react/jsx-runtime";
import { useState, useRef, useEffect, useId } from "react";
import { Collapsible, CollapsibleTrigger, CollapsibleContent, ShimmerText } from "@fabrials/ui";
import { ChevronDownIcon, BrainIcon, GlobeIcon, SearchIcon, FileTextIcon, ExternalLinkIcon } from "./chat-icons.js";
import { hostnameFromUrl, sourcePath, SourceFavicon } from "./citations.js";
function join(...values) {
  return values.filter(Boolean).join(" ");
}
function ActivityIcon({ icon, live, className }) {
  return /* @__PURE__ */ jsxs("span", { className: join("fui-activity-icon", className), "data-live": live || void 0, children: [
    icon,
    /* @__PURE__ */ jsx("span", { "aria-hidden": true, className: "fui-activity-dot" })
  ] });
}
function useControllable(prop, initial, onChange) {
  const [state, setState] = useState(initial);
  const value = prop === void 0 ? state : prop;
  const set = (next) => {
    setState(next);
    if (next !== value) onChange?.(next);
  };
  return [value, set];
}
function ActivityDisclosure({
  icon,
  live,
  label,
  children,
  open: openProp,
  defaultOpen = false,
  onOpenChange,
  toggleLabel,
  className
}) {
  const [open, setOpen] = useControllable(openProp, defaultOpen, onOpenChange);
  const text = /* @__PURE__ */ jsx("span", { className: "fui-activity-label", children: live ? /* @__PURE__ */ jsx(ShimmerText, { as: "span", duration: 1.6, children: label }) : label }, label);
  if (children == null || children === false) {
    return /* @__PURE__ */ jsx("div", { "aria-live": "polite", className: join("fui-activity", className), "data-live": live || void 0, role: "status", children: /* @__PURE__ */ jsxs("div", { className: "fui-activity-row", children: [
      /* @__PURE__ */ jsx(ActivityIcon, { icon, live }),
      text
    ] }) });
  }
  return /* @__PURE__ */ jsxs(
    Collapsible,
    {
      "aria-live": "polite",
      className: join("fui-activity", className),
      "data-live": live || void 0,
      onOpenChange: setOpen,
      open,
      role: "status",
      children: [
        /* @__PURE__ */ jsxs(
          CollapsibleTrigger,
          {
            "aria-label": toggleLabel ? `${label}. ${toggleLabel(open)}` : void 0,
            className: "fui-activity-row fui-activity-trigger",
            children: [
              /* @__PURE__ */ jsx(ActivityIcon, { icon, live }),
              text,
              /* @__PURE__ */ jsx(ChevronDownIcon, { className: "fui-disclosure-chevron" })
            ]
          }
        ),
        /* @__PURE__ */ jsx(CollapsibleContent, { className: "fui-activity-body", children })
      ]
    }
  );
}
function reasoningLabel(streaming, seconds, labels = {}) {
  if (streaming || seconds === 0) return labels.thinking ?? "Thinking…";
  if (seconds === void 0) return labels.brief ?? "Thought for a few seconds";
  return labels.thoughtFor?.(seconds) ?? `Thought for ${seconds}s`;
}
const AUTO_CLOSE_MS = 1e3;
function ReasoningDisclosure({
  streaming = false,
  durationSeconds,
  children,
  open: openProp,
  defaultOpen,
  onOpenChange,
  autoClose = true,
  labels,
  icon = /* @__PURE__ */ jsx(BrainIcon, {}),
  className
}) {
  const explicitlyClosed = defaultOpen === false;
  const [open, setOpen] = useControllable(openProp, defaultOpen ?? streaming, onOpenChange);
  const [measured, setMeasured] = useState(void 0);
  const startedAt = useRef(null);
  const everStreamed = useRef(streaming);
  const [autoClosed, setAutoClosed] = useState(false);
  useEffect(() => {
    if (streaming) {
      everStreamed.current = true;
      if (startedAt.current === null) startedAt.current = Date.now();
    } else if (startedAt.current !== null) {
      setMeasured(Math.ceil((Date.now() - startedAt.current) / 1e3));
      startedAt.current = null;
    }
  }, [streaming]);
  const openRef = useRef(setOpen);
  useEffect(() => {
    openRef.current = setOpen;
  });
  useEffect(() => {
    if (streaming && !open && !explicitlyClosed) openRef.current(true);
  }, [streaming, open, explicitlyClosed]);
  useEffect(() => {
    if (!autoClose || !everStreamed.current || streaming || !open || autoClosed) return;
    const timer = setTimeout(() => {
      openRef.current(false);
      setAutoClosed(true);
    }, AUTO_CLOSE_MS);
    return () => clearTimeout(timer);
  }, [autoClose, streaming, open, autoClosed]);
  const seconds = durationSeconds ?? measured;
  return /* @__PURE__ */ jsx(
    ActivityDisclosure,
    {
      className: join("fui-reasoning", className),
      icon,
      label: reasoningLabel(streaming, seconds, labels),
      live: streaming,
      onOpenChange: setOpen,
      open,
      children
    }
  );
}
function searchPhase(input) {
  if (!input.streaming) return "done";
  if (input.pending) return "searching";
  return input.isLast ? "reading" : "done";
}
function searchDoneLabel(sourceCount, searchCount = 0) {
  const bits = ["Searched the web"];
  if (searchCount > 0) bits.push(`${searchCount} ${searchCount === 1 ? "search" : "searches"}`);
  if (sourceCount > 0) bits.push(`${sourceCount} ${sourceCount === 1 ? "source" : "sources"}`);
  return bits.join(" · ");
}
function searchLabel(phase, sourceCount, searchCount = 0) {
  if (phase === "searching") return "Searching the web…";
  if (phase === "reading") return "Reading results…";
  return searchDoneLabel(sourceCount, searchCount);
}
function countSearches(steps) {
  return steps.filter((step) => step.type === "search").length;
}
function stepHost(url) {
  try {
    return new URL(url).hostname.replace(/^www\./, "");
  } catch {
    return url;
  }
}
function searchStepLabel(step) {
  return step.type === "search" ? step.query : step.title?.trim() || `Opened ${stepHost(step.url)}`;
}
function uniqueUrls(urls) {
  return [...new Set((urls ?? []).map((url) => url.trim()).filter(Boolean))];
}
function searchStepKindLabel(step) {
  return step.type === "search" ? "Searched web" : "Read page";
}
function SearchResults({
  id,
  urls,
  titleFor,
  faviconUrl
}) {
  return /* @__PURE__ */ jsx("ul", { className: "fui-search-results", id, children: urls.map((url) => {
    const host = hostnameFromUrl(url);
    const title = titleFor?.(url)?.trim();
    const path = sourcePath(url);
    return /* @__PURE__ */ jsx("li", { children: /* @__PURE__ */ jsxs("a", { className: "fui-search-result", href: url, rel: "noreferrer", target: "_blank", children: [
      /* @__PURE__ */ jsx("span", { className: "fui-search-result-title", children: title || host }),
      /* @__PURE__ */ jsxs("span", { className: "fui-search-result-meta", children: [
        /* @__PURE__ */ jsx(SourceFavicon, { faviconUrl, url }),
        /* @__PURE__ */ jsx("span", { className: "fui-search-result-host", children: host }),
        path && !title ? /* @__PURE__ */ jsx("span", { className: "fui-search-result-path", children: path }) : null
      ] })
    ] }) }, url);
  }) });
}
function SearchStepItem({
  step,
  last,
  live,
  titleFor,
  faviconUrl,
  id
}) {
  const [open, setOpen] = useState(false);
  const results = step.type === "search" ? uniqueUrls(step.sources) : [];
  const pageTitle = step.type === "open_page" ? step.title?.trim() || titleFor?.(step.url)?.trim() : void 0;
  const label = step.type === "open_page" && pageTitle ? pageTitle : searchStepLabel(step);
  const kind = searchStepKindLabel(step);
  const icon = step.type === "search" ? /* @__PURE__ */ jsx(SearchIcon, {}) : /* @__PURE__ */ jsx(FileTextIcon, {});
  const text = /* @__PURE__ */ jsxs("span", { className: "fui-search-step-text", children: [
    /* @__PURE__ */ jsx("span", { className: "fui-search-step-kind", children: kind }),
    live ? /* @__PURE__ */ jsx(ShimmerText, { as: "span", className: "fui-search-step-query", children: label }) : /* @__PURE__ */ jsx("span", { className: "fui-search-step-query", title: label, children: label })
  ] });
  const count = results.length > 0 ? /* @__PURE__ */ jsx("span", { className: "fui-search-step-count", children: results.length }) : null;
  return /* @__PURE__ */ jsxs("li", { className: "fui-search-step", "data-last": last || void 0, "data-live": live || void 0, "data-open": open || void 0, children: [
    /* @__PURE__ */ jsx("span", { "aria-hidden": true, className: "fui-search-step-rail", children: /* @__PURE__ */ jsxs("span", { className: "fui-search-step-badge", children: [
      /* @__PURE__ */ jsx("span", { className: "fui-search-step-icon", children: icon }),
      results.length > 0 ? /* @__PURE__ */ jsx("span", { className: "fui-search-step-chevron", children: /* @__PURE__ */ jsx(ChevronDownIcon, {}) }) : null
    ] }) }),
    /* @__PURE__ */ jsxs("div", { className: "fui-search-step-main", children: [
      results.length > 0 ? /* @__PURE__ */ jsxs(
        "button",
        {
          "aria-controls": `${id}-results`,
          "aria-expanded": open,
          className: "fui-search-step-head",
          onClick: () => setOpen((value) => !value),
          type: "button",
          children: [
            text,
            count
          ]
        }
      ) : step.type === "open_page" ? /* @__PURE__ */ jsxs("a", { className: "fui-search-step-head", href: step.url, rel: "noreferrer", target: "_blank", title: step.url, children: [
        text,
        /* @__PURE__ */ jsx("span", { className: "fui-search-step-open", children: /* @__PURE__ */ jsx(ExternalLinkIcon, {}) })
      ] }) : /* @__PURE__ */ jsx("div", { className: "fui-search-step-head", children: text }),
      open && results.length > 0 ? /* @__PURE__ */ jsx(SearchResults, { faviconUrl, id: `${id}-results`, titleFor, urls: results }) : null
    ] })
  ] });
}
function SearchStepList({ steps, live = false, titleFor, faviconUrl }) {
  const uid = useId().replace(/:/g, "");
  return /* @__PURE__ */ jsx("ol", { className: "fui-search-steps", children: steps.map((step, index) => /* @__PURE__ */ jsx(
    SearchStepItem,
    {
      faviconUrl,
      id: `fui-search-${uid}-${index}`,
      last: index === steps.length - 1,
      live: live && index === steps.length - 1,
      step,
      titleFor
    },
    `${step.type}-${step.type === "search" ? step.query : step.url}-${index}`
  )) });
}
function SearchStepsDisclosure({
  phase,
  steps = [],
  sourceCount = 0,
  label,
  icon = /* @__PURE__ */ jsx(GlobeIcon, {}),
  defaultOpen,
  className,
  titleFor,
  faviconUrl
}) {
  const text = label ?? searchLabel(phase, sourceCount, countSearches(steps));
  return /* @__PURE__ */ jsx(
    ActivityDisclosure,
    {
      className: join("fui-search-activity", className),
      defaultOpen,
      icon,
      label: text,
      live: phase !== "done",
      toggleLabel: (open) => `${open ? "Hide" : "Show"} search queries`,
      children: steps.length > 0 ? /* @__PURE__ */ jsx(SearchStepList, { faviconUrl, live: phase !== "done", steps, titleFor }) : null
    }
  );
}
export {
  ActivityDisclosure,
  ActivityIcon,
  ReasoningDisclosure,
  SearchStepList,
  SearchStepsDisclosure,
  countSearches,
  reasoningLabel,
  searchDoneLabel,
  searchLabel,
  searchPhase,
  searchStepKindLabel,
  searchStepLabel
};
