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
function Toaster({ theme, className, style, ...props }) {
  const detected = useDocumentTheme();
  return /* @__PURE__ */ jsx(
    Toaster$1,
    {
      theme: theme ?? detected,
      className: ["fui-toaster", "toaster", "group", className].filter(Boolean).join(" "),
      icons: {
        success: /* @__PURE__ */ jsx(CircleCheckIcon, { "aria-hidden": true, className: "fui-toast-icon", "data-tone": "success" }),
        info: /* @__PURE__ */ jsx(InfoIcon, { "aria-hidden": true, className: "fui-toast-icon", "data-tone": "info" }),
        warning: /* @__PURE__ */ jsx(TriangleAlertIcon, { "aria-hidden": true, className: "fui-toast-icon", "data-tone": "warning" }),
        error: /* @__PURE__ */ jsx(OctagonXIcon, { "aria-hidden": true, className: "fui-toast-icon", "data-tone": "danger" }),
        loading: /* @__PURE__ */ jsx(LoaderCircle, { "aria-hidden": true, className: "fui-toast-icon fui-spin" })
      },
      style: {
        "--normal-bg": "var(--popover)",
        "--normal-text": "var(--popover-foreground)",
        "--normal-border": "var(--border)",
        "--border-radius": "var(--fui-radius-lg)",
        ...style
      },
      ...props
    }
  );
}
export {
  Toaster,
  toast
};
