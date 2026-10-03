"use client";
import { jsxs, jsx } from "react/jsx-runtime";
import { useState, useCallback, useMemo, useContext, useEffect, createContext } from "react";
import { HoverCard, HoverCardTrigger, HoverCardContent, Collapsible, CollapsibleTrigger, CollapsibleContent } from "@fabrials/ui";
import { ChevronDownIcon } from "./chat-icons.js";
const previewCache = /* @__PURE__ */ new Map();
function loadPreview(loader, url) {
  let pending = previewCache.get(url);
  if (!pending) {
    pending = loader(url).catch(() => null);
    previewCache.set(url, pending);
  }
  return pending;
}
function useLinkPreview(url, loader, enabled = true) {
  const [state, setState] = useState({
    preview: null,
    loading: false
  });
  useEffect(() => {
    if (!url || !loader || !enabled) return;
    let alive = true;
    setState({ url, preview: null, loading: true });
    void loadPreview(loader, url).then((preview) => {
      if (alive) setState({ url, preview, loading: false });
    });
    return () => {
      alive = false;
    };
  }, [url, loader, enabled]);
  if (state.url !== url) return { preview: null, loading: Boolean(url && loader && enabled) };
  return state;
}
function hostnameFromUrl(value) {
  try {
    return new URL(value).hostname.replace(/^www\./, "");
  } catch {
    return value;
  }
}
function sourcePath(value) {
  try {
    const { pathname, search } = new URL(value);
    const path = (pathname + search).replace(/\/+$/, "");
    if (!path) return "";
    try {
      return decodeURIComponent(path);
    } catch {
      return path;
    }
  } catch {
    return "";
  }
}
function sourceLabel(source) {
  const title = source.title?.trim();
  return title && !/^\d+$/.test(title) ? title : hostnameFromUrl(source.url);
}
const CitationContext = createContext(null);
function numbered(sources) {
  if (!sources) return /* @__PURE__ */ new Map();
  if (sources instanceof Map) return sources;
  const map = /* @__PURE__ */ new Map();
  sources.forEach(
    (source, index) => map.set(source.number ?? index + 1, source)
  );
  return map;
}
function CitationProvider({
  sources,
  faviconUrl,
  previewLoader,
  children
}) {
  const [active, setActiveState] = useState(null);
  const setActive = useCallback((n) => setActiveState(n), []);
  const clearActive = useCallback(
    (n) => setActiveState((current) => current === n ? null : current),
    []
  );
  const map = useMemo(() => numbered(sources), [sources]);
  const value = useMemo(
    () => ({ sources: map, active, faviconUrl, previewLoader, setActive, clearActive }),
    [map, active, faviconUrl, previewLoader, setActive, clearActive]
  );
  return /* @__PURE__ */ jsx(CitationContext.Provider, { value, children });
}
const NOOP = {
  onMouseEnter: () => {
  },
  onMouseLeave: () => {
  },
  onFocus: () => {
  },
  onBlur: () => {
  }
};
function useCitation(n) {
  const ctx = useContext(CitationContext);
  const setActive = ctx?.setActive;
  const clearActive = ctx?.clearActive;
  useEffect(() => {
    if (n == null || !clearActive) return;
    return () => clearActive(n);
  }, [n, clearActive]);
  const handlers = useMemo(() => {
    if (n == null || !setActive || !clearActive) return NOOP;
    const enter = () => setActive(n);
    const leave = () => clearActive(n);
    return { onMouseEnter: enter, onMouseLeave: leave, onFocus: enter, onBlur: leave };
  }, [n, setActive, clearActive]);
  return {
    active: n != null && ctx?.active === n,
    handlers,
    source: n == null ? void 0 : ctx?.sources.get(n),
    faviconUrl: ctx?.faviconUrl,
    previewLoader: ctx?.previewLoader
  };
}
function join(...values) {
  return values.filter(Boolean).join(" ");
}
function SourceFavicon({
  url,
  faviconUrl,
  className
}) {
  const ctx = useContext(CitationContext);
  const resolve = faviconUrl ?? ctx?.faviconUrl;
  const src = resolve?.(url) ?? null;
  const [failed, setFailed] = useState(null);
  if (!src || failed === src) {
    const letter = hostnameFromUrl(url).replace(/[^\p{L}\p{N}]/gu, "").charAt(0).toUpperCase() || "?";
    return /* @__PURE__ */ jsx("span", { "aria-hidden": true, className: join("fui-source-favicon", "fui-source-favicon-letter", className), children: letter });
  }
  return /* @__PURE__ */ jsx(
    "img",
    {
      alt: "",
      "aria-hidden": true,
      className: join("fui-source-favicon", className),
      height: 16,
      loading: "lazy",
      onError: () => setFailed(src),
      src,
      width: 16
    }
  );
}
function CitationChip({
  number,
  href,
  source,
  active,
  faviconUrl,
  previewLoader,
  delay = 200,
  className,
  onMouseEnter,
  onMouseLeave,
  onFocus,
  onBlur,
  ...rest
}) {
  const citation = useCitation(number);
  const resolved = source ?? citation.source ?? {};
  const label = sourceLabel({ url: href, title: resolved.title });
  const host = hostnameFromUrl(href);
  const isActive = active ?? citation.active;
  const icon = faviconUrl ?? citation.faviconUrl;
  return /* @__PURE__ */ jsxs(HoverCard, { children: [
    /* @__PURE__ */ jsx(
      HoverCardTrigger,
      {
        delay,
        render: /* @__PURE__ */ jsx(
          "a",
          {
            ...rest,
            "aria-label": `Source ${number}: ${label}`,
            className: join("fui-citation-chip", className),
            "data-active": isActive || void 0,
            "data-citation": number,
            href,
            onBlur: (event) => {
              citation.handlers.onBlur();
              onBlur?.(event);
            },
            onFocus: (event) => {
              citation.handlers.onFocus();
              onFocus?.(event);
            },
            onMouseEnter: (event) => {
              citation.handlers.onMouseEnter();
              onMouseEnter?.(event);
            },
            onMouseLeave: (event) => {
              citation.handlers.onMouseLeave();
              onMouseLeave?.(event);
            },
            rel: "noreferrer",
            target: "_blank"
          }
        ),
        children: number
      }
    ),
    /* @__PURE__ */ jsx(HoverCardContent, { align: "start", className: "fui-citation-card", side: "top", children: /* @__PURE__ */ jsx(
      LinkPreviewCard,
      {
        faviconUrl: icon,
        label,
        meta: number != null ? `Source ${number} · ${host}` : host,
        previewLoader: previewLoader ?? citation.previewLoader,
        url: href
      }
    ) })
  ] });
}
function LinkPreviewCard({
  url,
  label,
  meta,
  faviconUrl,
  previewLoader
}) {
  const { preview, loading } = useLinkPreview(url, previewLoader);
  const [brokenImage, setBrokenImage] = useState(null);
  const host = hostnameFromUrl(url);
  const title = preview?.title?.trim() || label || host;
  const image = preview?.image && brokenImage !== preview.image ? preview.image : null;
  return /* @__PURE__ */ jsxs("a", { className: "fui-citation-card-link", "data-loading": loading || void 0, href: url, rel: "noreferrer", target: "_blank", children: [
    image ? /* @__PURE__ */ jsx(
      "img",
      {
        alt: "",
        className: "fui-link-preview-image",
        loading: "lazy",
        onError: () => setBrokenImage(image),
        referrerPolicy: "no-referrer",
        src: image
      }
    ) : null,
    /* @__PURE__ */ jsxs("span", { className: "fui-link-preview-body", children: [
      /* @__PURE__ */ jsxs("span", { className: "fui-link-preview-site", children: [
        /* @__PURE__ */ jsx(SourceFavicon, { faviconUrl, url }),
        /* @__PURE__ */ jsx("span", { children: preview?.siteName?.trim() || meta || host })
      ] }),
      /* @__PURE__ */ jsx("span", { className: "fui-citation-card-title", children: title }),
      preview?.description ? /* @__PURE__ */ jsx("span", { className: "fui-link-preview-description", children: preview.description }) : loading ? /* @__PURE__ */ jsx("span", { "aria-hidden": true, className: "fui-link-preview-skeleton" }) : null,
      preview?.siteName && meta ? /* @__PURE__ */ jsx("span", { className: "fui-citation-card-meta", children: meta }) : null
    ] })
  ] });
}
function LinkWithPreview({ href, previewLoader, faviconUrl, delay = 300, children, ...props }) {
  const ctx = useContext(CitationContext);
  const loader = previewLoader ?? ctx?.previewLoader;
  const icon = faviconUrl ?? ctx?.faviconUrl;
  if (!loader || !/^https?:/i.test(href)) {
    return /* @__PURE__ */ jsx("a", { ...props, href, rel: "noreferrer", target: "_blank", children });
  }
  return /* @__PURE__ */ jsxs(HoverCard, { children: [
    /* @__PURE__ */ jsx(HoverCardTrigger, { delay, render: /* @__PURE__ */ jsx("a", { ...props, href, rel: "noreferrer", target: "_blank" }), children }),
    /* @__PURE__ */ jsx(HoverCardContent, { align: "start", className: "fui-citation-card", side: "top", children: /* @__PURE__ */ jsx(LinkPreviewCard, { faviconUrl: icon, previewLoader: loader, url: href }) })
  ] });
}
function SourceCard({
  url,
  title,
  number,
  active,
  faviconUrl,
  previewLoader,
  className,
  onMouseEnter,
  onMouseLeave,
  onFocus,
  onBlur,
  ...props
}) {
  const citation = useCitation(number);
  const isActive = active ?? citation.active;
  const host = hostnameFromUrl(url);
  const { preview } = useLinkPreview(url, previewLoader ?? citation.previewLoader);
  const label = preview?.title?.trim() || sourceLabel({ url, title });
  const secondary = label === host ? sourcePath(url) : host;
  return /* @__PURE__ */ jsxs(
    "a",
    {
      ...props,
      className: join("fui-source-card", className),
      "data-active": isActive || void 0,
      "data-citation": number,
      href: url,
      onBlur: (event) => {
        citation.handlers.onBlur();
        onBlur?.(event);
      },
      onFocus: (event) => {
        citation.handlers.onFocus();
        onFocus?.(event);
      },
      onMouseEnter: (event) => {
        citation.handlers.onMouseEnter();
        onMouseEnter?.(event);
      },
      onMouseLeave: (event) => {
        citation.handlers.onMouseLeave();
        onMouseLeave?.(event);
      },
      rel: "noreferrer",
      target: "_blank",
      title: `${label} — ${url}`,
      children: [
        typeof number === "number" ? /* @__PURE__ */ jsx("span", { "aria-label": `Source ${number}`, className: "fui-source-number", children: number }) : null,
        /* @__PURE__ */ jsx(SourceFavicon, { faviconUrl: faviconUrl ?? citation.faviconUrl, url }),
        /* @__PURE__ */ jsx("span", { className: "fui-source-title", children: label }),
        secondary ? /* @__PURE__ */ jsx("span", { className: "fui-source-meta", children: secondary }) : null
      ]
    }
  );
}
function sourcesLabel(count) {
  return `${count} ${count === 1 ? "source" : "sources"}`;
}
function Sources({
  sources,
  defaultOpen = false,
  open: openProp,
  onOpenChange,
  faviconUrl,
  label = sourcesLabel,
  previewCount = 4,
  className
}) {
  const [openState, setOpenState] = useState(defaultOpen);
  const open = openProp ?? openState;
  if (sources.length === 0) return null;
  const preview = sources.slice(0, previewCount);
  return /* @__PURE__ */ jsxs(
    Collapsible,
    {
      className: join("fui-sources", className),
      onOpenChange: (next) => {
        setOpenState(next);
        onOpenChange?.(next);
      },
      open,
      children: [
        /* @__PURE__ */ jsxs(CollapsibleTrigger, { className: "fui-sources-trigger", children: [
          preview.length > 0 ? /* @__PURE__ */ jsx("span", { className: "fui-sources-preview", children: preview.map((source, index) => /* @__PURE__ */ jsx("span", { className: "fui-sources-preview-item", children: /* @__PURE__ */ jsx(SourceFavicon, { faviconUrl, url: source.url }) }, `${source.url}-${index}`)) }) : null,
          /* @__PURE__ */ jsx("span", { children: label(sources.length) }),
          /* @__PURE__ */ jsx(ChevronDownIcon, { className: "fui-disclosure-chevron" })
        ] }),
        /* @__PURE__ */ jsx(CollapsibleContent, { className: "fui-sources-grid", render: /* @__PURE__ */ jsx("ol", {}), children: sources.map((source, index) => /* @__PURE__ */ jsx("li", { children: /* @__PURE__ */ jsx(
          SourceCard,
          {
            faviconUrl,
            number: source.number ?? index + 1,
            title: source.title,
            url: source.url
          }
        ) }, `${source.url}-${index}`)) })
      ]
    }
  );
}
export {
  CitationChip,
  CitationProvider,
  LinkPreviewCard,
  LinkWithPreview,
  SourceCard,
  SourceFavicon,
  Sources,
  hostnameFromUrl,
  sourceLabel,
  sourcePath,
  sourcesLabel,
  useCitation,
  useLinkPreview
};
