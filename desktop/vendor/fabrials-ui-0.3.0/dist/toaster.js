"use client";
import { jsx } from "react/jsx-runtime";
import { useState, useEffect } from "react";
import { Toaster as Toaster$1 } from "sonner";
import { toast } from "sonner";
import { Loader2Icon, OctagonXIcon, TriangleAlertIcon, InfoIcon, CircleCheckIcon } from "lucide-react";
function useDocumentTheme() {
  const [theme, setTheme] = useState("light");
  useEffect(() => {
    const root = document.documentElement;
    const read = () => setTheme(root.classList.contains("dark") ? "dark" : "light");
    read();
    const observer = new MutationObserver(read);
    observer.observe(root, { attributes: true, attributeFilter: ["class"] });
    return () => observer.disconnect();
  }, []);
  return theme;
}
function Toaster({ theme, ...props }) {
  const detected = useDocumentTheme();
  return /* @__PURE__ */ jsx(
    Toaster$1,
    {
      theme: theme ?? detected,
      className: "toaster group",
      icons: {
        success: /* @__PURE__ */ jsx(CircleCheckIcon, { className: "size-4" }),
        info: /* @__PURE__ */ jsx(InfoIcon, { className: "size-4" }),
        warning: /* @__PURE__ */ jsx(TriangleAlertIcon, { className: "size-4" }),
        error: /* @__PURE__ */ jsx(OctagonXIcon, { className: "size-4" }),
        loading: /* @__PURE__ */ jsx(Loader2Icon, { className: "size-4 animate-spin" })
      },
      style: {
        "--normal-bg": "var(--popover)",
        "--normal-text": "var(--popover-foreground)",
        "--normal-border": "var(--border)",
        "--border-radius": "var(--radius)"
      },
      ...props
    }
  );
}
export {
  Toaster,
  toast
};
