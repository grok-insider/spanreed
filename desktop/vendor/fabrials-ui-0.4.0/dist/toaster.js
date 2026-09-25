"use client";
import { jsx } from "react/jsx-runtime";
import { useState, useEffect } from "react";
import { Toaster as Toaster$1 } from "sonner";
import { toast } from "sonner";
import { LoaderCircle, OctagonXIcon, TriangleAlertIcon, InfoIcon, CircleCheckIcon } from "lucide-react";
function useDocumentTheme() {
  const [theme, setTheme] = useState("light");
  useEffect(() => {
    const root = document.documentElement;
    const read = () => setTheme(
      root.classList.contains("dark") || root.dataset.theme === "dark" ? "dark" : "light"
    );
    read();
    const observer = new MutationObserver(read);
    observer.observe(root, {
      attributes: true,
      attributeFilter: ["class", "data-theme"]
    });
    return () => observer.disconnect();
  }, []);
  return theme;
}
function Toaster({
  theme,
  className,
  style,
  expand,
  closeButton,
  gap,
  richColors: _richColors,
  icons: _icons,
  ...props
}) {
  const detected = useDocumentTheme();
  return /* @__PURE__ */ jsx(
    Toaster$1,
    {
      theme: theme ?? detected,
      expand: expand ?? true,
      closeButton: closeButton ?? true,
      gap: gap ?? 12,
      className: ["fui-toaster", className].filter(Boolean).join(" "),
      icons: {
        success: /* @__PURE__ */ jsx(CircleCheckIcon, { "aria-hidden": true, className: "fui-toast-icon", "data-tone": "success" }),
        info: /* @__PURE__ */ jsx(InfoIcon, { "aria-hidden": true, className: "fui-toast-icon", "data-tone": "info" }),
        warning: /* @__PURE__ */ jsx(TriangleAlertIcon, { "aria-hidden": true, className: "fui-toast-icon", "data-tone": "warning" }),
        error: /* @__PURE__ */ jsx(OctagonXIcon, { "aria-hidden": true, className: "fui-toast-icon", "data-tone": "danger" }),
        loading: /* @__PURE__ */ jsx(LoaderCircle, { "aria-hidden": true, className: "fui-toast-icon fui-spin" })
      },
      style: {
        ...style,
        "--normal-bg": "var(--popover)",
        "--normal-text": "var(--foreground)",
        "--normal-border": "var(--border)",
        "--border-radius": "var(--fui-radius-xl)",
        "--success-bg": "color-mix(in oklab, var(--success) 9%, var(--popover))",
        "--success-border": "color-mix(in oklab, var(--fui-success-ink) 30%, var(--border))",
        "--success-text": "var(--foreground)",
        "--error-bg": "color-mix(in oklab, var(--destructive) 8%, var(--popover))",
        "--error-border": "color-mix(in oklab, var(--fui-danger-ink) 40%, var(--border))",
        "--error-text": "var(--foreground)",
        "--warning-bg": "color-mix(in oklab, var(--warning) 11%, var(--popover))",
        "--warning-border": "color-mix(in oklab, var(--fui-warning-ink) 32%, var(--border))",
        "--warning-text": "var(--foreground)",
        "--info-bg": "color-mix(in oklab, var(--brand) 8%, var(--popover))",
        "--info-border": "color-mix(in oklab, var(--brand-ink) 30%, var(--border))",
        "--info-text": "var(--foreground)"
      },
      ...props
    }
  );
}
export {
  Toaster,
  toast
};
