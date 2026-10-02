"use client";
import { jsxs, jsx } from "react/jsx-runtime";
import { Card, CardHeader, Badge, CardContent, Progress, Label, NativeCheckbox, Input, Button } from "@fabrials/ui";
import * as React from "react";
import { ProviderIcon } from "./provider-icon.js";
import { validTimestamp, resetExpirySummary, upcomingResetExpiries, resetExpiryLabel } from "./reset-expiry.js";
function RoutingExplanation() {
  return /* @__PURE__ */ jsx("p", { className: "fb-muted", children: "Autosteer selects an eligible account within the same provider. Pinned account routes keep their selected account. It does not switch models or providers." });
}
function RoutingPolicyForm({
  provider,
  enabled,
  threshold,
  pending,
  onSave
}) {
  const [on, setOn] = React.useState(enabled);
  const [limit, setLimit] = React.useState(String(threshold));
  const numeric = Number(limit);
  const valid = limit.trim() !== "" && Number.isFinite(numeric) && numeric > 0 && numeric <= 100;
  const changed = on !== enabled || numeric !== threshold;
  return /* @__PURE__ */ jsxs(
    "form",
    {
      className: "fb-form",
      onSubmit: (event) => {
        event.preventDefault();
        if (valid && !pending) void onSave(on, numeric);
      },
      children: [
        /* @__PURE__ */ jsx(Label, { children: /* @__PURE__ */ jsxs("span", { children: [
          /* @__PURE__ */ jsx(
            NativeCheckbox,
            {
              type: "checkbox",
              checked: on,
              disabled: pending,
              onChange: (event) => setOn(event.target.checked)
            }
          ),
          " ",
          "Enable ",
          provider,
          " autosteer"
        ] }) }),
        /* @__PURE__ */ jsx("p", { className: "fb-muted", children: on ? "The highest-ranked eligible account receives the next unpinned request." : "Unpinned requests use the active account." }),
        /* @__PURE__ */ jsxs(Label, { children: [
          "Skip accounts at quota usage (%)",
          /* @__PURE__ */ jsx(
            Input,
            {
              type: "number",
              min: "0.01",
              max: "100",
              step: "any",
              required: true,
              value: limit,
              disabled: pending,
              onChange: (event) => setLimit(event.target.value)
            }
          )
        ] }),
        /* @__PURE__ */ jsx(
          Button,
          {
            className: " ",
            disabled: pending || !valid || !changed,
            type: "submit",
            children: pending ? "Saving…" : "Save routing preferences"
          }
        )
      ]
    }
  );
}
let clock = null;
const clockListeners = /* @__PURE__ */ new Set();
let clockTimer;
function subscribeClock(listener) {
  clockListeners.add(listener);
  clock = Date.now();
  if (!clockTimer)
    clockTimer = setInterval(() => {
      clock = Date.now();
      for (const notify of clockListeners) notify();
    }, 6e4);
  return () => {
    clockListeners.delete(listener);
    if (!clockListeners.size) {
      clearInterval(clockTimer);
      clockTimer = void 0;
    }
  };
}
function useClock() {
  return React.useSyncExternalStore(
    subscribeClock,
    () => clock,
    () => null
  );
}
function ResetInventory({
  observation
}) {
  const now = useClock();
  if (!observation || observation.availability === "unsupported") return null;
  const inventory = observation.value;
  const expirySummary = now === null ? null : resetExpirySummary(observation, now);
  const upcoming = upcomingResetExpiries(inventory, now);
  const lastReported = now === null || observation.availability !== "available" || expirySummary?.stale || expirySummary?.expired;
  return /* @__PURE__ */ jsxs("section", { className: "fb-observation", "aria-label": "Limit reset credits", children: [
    /* @__PURE__ */ jsxs("div", { className: "fb-row", children: [
      /* @__PURE__ */ jsx("span", { className: "fb-label", children: "Limit reset credits" }),
      /* @__PURE__ */ jsx("strong", { children: inventory ? `${inventory.available} ${lastReported ? "last reported" : "available"}` : "Unavailable" })
    ] }),
    upcoming.length > 0 && /* @__PURE__ */ jsxs("div", { className: "fb-reset-preview", children: [
      /* @__PURE__ */ jsxs(
        "svg",
        {
          "aria-hidden": "true",
          width: "17",
          height: "17",
          viewBox: "0 0 24 24",
          fill: "none",
          stroke: "currentColor",
          strokeWidth: "1.6",
          children: [
            /* @__PURE__ */ jsx("circle", { cx: "12", cy: "12", r: "9" }),
            /* @__PURE__ */ jsx("path", { d: "M12 6v6H8" })
          ]
        }
      ),
      /* @__PURE__ */ jsx("ul", { "aria-label": "Next reported reset credit expiries", children: upcoming.map((expiry, index) => /* @__PURE__ */ jsx("li", { children: /* @__PURE__ */ jsx(
        "time",
        {
          dateTime: new Date(expiry).toISOString(),
          title: new Date(expiry).toLocaleString(),
          children: resetExpiryLabel(expiry, now)
        }
      ) }, `${expiry}-${index}`)) })
    ] }),
    !!expirySummary?.expiring && /* @__PURE__ */ jsxs("p", { className: "fb-reset-warning", role: "status", children: [
      expirySummary.expiring,
      " reset credit",
      expirySummary.expiring === 1 ? " expires" : "s expire",
      " within 24 hours."
    ] }),
    !!expirySummary?.expired && /* @__PURE__ */ jsx("p", { className: "fb-muted", role: "status", children: "A reported expiry has passed. Refresh inventory to check remaining credits." }),
    expirySummary?.stale && inventory && /* @__PURE__ */ jsx("p", { className: "fb-muted", children: "Refresh inventory to check current availability." }),
    inventory && inventory.credits.length > 0 && /* @__PURE__ */ jsxs("details", { className: "fb-reset-details", children: [
      /* @__PURE__ */ jsx("summary", { children: "View expiry dates" }),
      /* @__PURE__ */ jsx("ul", { children: inventory.credits.map((credit, index) => {
        const expiry = validTimestamp(credit.expiresAtMs) ? credit.expiresAtMs : null;
        return /* @__PURE__ */ jsxs("li", { className: "fb-row", children: [
          /* @__PURE__ */ jsxs("span", { children: [
            "Reset ",
            index + 1
          ] }),
          /* @__PURE__ */ jsx("span", { children: expiry === null ? "Expiry unavailable" : /* @__PURE__ */ jsx(
            "time",
            {
              dateTime: new Date(expiry).toISOString(),
              title: now === null ? void 0 : new Date(expiry).toLocaleString(),
              children: resetExpiryLabel(expiry, now)
            }
          ) })
        ] }, index);
      }) })
    ] }),
    inventory && !inventory.detailsComplete && /* @__PURE__ */ jsx("p", { className: "fb-muted", children: "Some expiry dates are unavailable." }),
    /* @__PURE__ */ jsx(ObservationStatus, { observation })
  ] });
}
function ObservationStatus({
  observation
}) {
  return /* @__PURE__ */ jsxs("div", { className: "fb-observation-status", children: [
    observation.freshness === "stale" && /* @__PURE__ */ jsx("span", { children: "Last known reading · " }),
    validTimestamp(observation.observedAtMs) && /* @__PURE__ */ jsxs("time", { dateTime: new Date(observation.observedAtMs).toISOString(), children: [
      "Updated",
      " ",
      new Date(observation.observedAtMs).toLocaleTimeString([], {
        hour: "2-digit",
        minute: "2-digit"
      })
    ] }),
    observation.error && /* @__PURE__ */ jsx("p", { children: observation.error })
  ] });
}
const dollars = (value) => value === null ? "Not reported" : new Intl.NumberFormat("en-US", {
  style: "currency",
  currency: "USD"
}).format(value);
function BalanceCard({
  observation
}) {
  if (!observation) return null;
  const value = observation.value;
  return /* @__PURE__ */ jsxs("section", { className: "fb-observation", "aria-label": "Credit balance", children: [
    /* @__PURE__ */ jsxs("div", { className: "fb-row", children: [
      /* @__PURE__ */ jsx("span", { className: "fb-label", children: "Available balance" }),
      /* @__PURE__ */ jsx("strong", { children: dollars(value?.remainingUsd ?? null) })
    ] }),
    value?.subscriptionRemainingUsd != null && /* @__PURE__ */ jsxs("div", { className: "fb-row fb-muted", children: [
      /* @__PURE__ */ jsx("span", { children: "Subscription" }),
      /* @__PURE__ */ jsxs("span", { children: [
        dollars(value.subscriptionRemainingUsd),
        value.subscriptionLimitUsd != null ? ` / ${dollars(value.subscriptionLimitUsd)}` : ""
      ] })
    ] }),
    value?.purchasedRemainingUsd != null && /* @__PURE__ */ jsxs("div", { className: "fb-row fb-muted", children: [
      /* @__PURE__ */ jsx("span", { children: "Purchased credits" }),
      /* @__PURE__ */ jsx("span", { children: dollars(value.purchasedRemainingUsd) })
    ] }),
    /* @__PURE__ */ jsx(ObservationStatus, { observation })
  ] });
}
function metricText(line) {
  return line.type === "text" ? line.value : line.type === "badge" ? line.text : "";
}
function ProviderCard({ provider }) {
  const lines = provider.lines.filter(
    (line) => line.type !== "barChart" && !(provider.resetInventory && (line.label.startsWith("Reset") || line.label === "Limit reset credits"))
  );
  return /* @__PURE__ */ jsxs(Card, { className: "fb-card-layout", children: [
    /* @__PURE__ */ jsxs(CardHeader, { className: "fb-row", children: [
      /* @__PURE__ */ jsxs("div", { children: [
        /* @__PURE__ */ jsx("h2", { children: provider.displayName }),
        /* @__PURE__ */ jsx(Badge, { children: provider.plan || "Connected provider" })
      ] }),
      /* @__PURE__ */ jsx(ProviderIcon, { provider: provider.providerId })
    ] }),
    /* @__PURE__ */ jsxs(CardContent, { className: "fb-content-layout", children: [
      lines.map(
        (line, index) => line.type === "progress" ? /* @__PURE__ */ jsxs("div", { className: "fb-metric", children: [
          /* @__PURE__ */ jsxs("div", { className: "fb-row", children: [
            /* @__PURE__ */ jsx("span", { children: line.label }),
            /* @__PURE__ */ jsx("strong", { children: line.format?.kind === "dollars" ? dollars(line.used ?? null) : `${Math.round(line.used ?? 0)}${line.format?.kind === "percent" ? "%" : ""}` })
          ] }),
          /* @__PURE__ */ jsx(
            Progress,
            {
              "aria-label": line.label,
              max: Math.max(1, line.limit ?? 100),
              value: line.used ?? 0
            }
          ),
          line.resetsAt && /* @__PURE__ */ jsxs("p", { className: "fb-muted", children: [
            "Resets",
            " ",
            /* @__PURE__ */ jsx("time", { dateTime: line.resetsAt, children: new Date(line.resetsAt).toLocaleString() })
          ] })
        ] }, index) : line.kind === "error" ? /* @__PURE__ */ jsx("p", { role: "status", className: "fb-error", children: metricText(line) }, index) : /* @__PURE__ */ jsxs("div", { className: "fb-row", children: [
          /* @__PURE__ */ jsx("span", { className: "fb-muted", children: line.label }),
          /* @__PURE__ */ jsx("span", { children: metricText(line) })
        ] }, index)
      ),
      !lines.length && /* @__PURE__ */ jsx("p", { className: "fb-muted", children: "No quota reported by this provider." }),
      /* @__PURE__ */ jsx(ResetInventory, { observation: provider.resetInventory })
    ] })
  ] });
}
export {
  BalanceCard,
  ObservationStatus,
  ProviderCard,
  ResetInventory,
  RoutingExplanation,
  RoutingPolicyForm,
  useClock
};
