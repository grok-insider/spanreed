"use client";
import { jsxs, Fragment, jsx } from "react/jsx-runtime";
import { Button, Label, NativeSelect } from "@fabrials/ui";
import * as React from "react";
import { ConsumptionView } from "./consumption.js";
const empty = (key) => ({
  key,
  tokens: 0,
  unknown_token_records: 0,
  known_usd: 0,
  partial: false,
  records: 0
});
function add(target, value) {
  target.tokens += value.tokens;
  target.unknown_token_records += value.unknown_token_records;
  target.known_usd += value.known_usd;
  target.partial ||= value.partial;
}
function project(snapshots) {
  const daily = /* @__PURE__ */ new Map();
  const clients = [], periods = [];
  const total = empty("total");
  for (const snapshot of snapshots) {
    const client = empty(snapshot.source);
    client.partial = snapshot.partial;
    for (const day of snapshot.days) {
      const row = {
        ...empty(day.date),
        tokens: day.tokens,
        known_usd: day.estimated_usd,
        partial: snapshot.partial
      };
      const date = daily.get(day.date) || empty(day.date);
      add(date, row);
      daily.set(day.date, date);
      add(client, row);
    }
    if (snapshot.period_totals) {
      const period = {
        ...empty(snapshot.source),
        tokens: snapshot.period_totals.tokens || 0,
        unknown_token_records: snapshot.period_totals.tokens === null ? 1 : 0,
        known_usd: snapshot.period_totals.known_usd,
        partial: snapshot.period_totals.partial
      };
      periods.push(period);
      add(client, period);
    }
    clients.push(client);
    add(total, client);
  }
  return {
    revision: Math.max(0, ...snapshots.map((s) => s.revision)),
    sources: snapshots.map((s) => ({
      source: s.source,
      client: s.source,
      revision: s.revision,
      detail: null,
      state: s.partial ? "partial" : "ready",
      records: s.days.length + (s.period_totals ? 1 : 0),
      last_success_ms: s.observed_at_ms
    })),
    total,
    daily: [...daily.values()].sort((a, b) => a.key.localeCompare(b.key)),
    period_totals: periods,
    clients,
    models: [],
    sessions: [],
    projects: []
  };
}
function SynchronizedConsumption({
  load
}) {
  const [snapshots, setSnapshots] = React.useState([]);
  const [device, setDevice] = React.useState("");
  const [error, setError] = React.useState(null);
  const [busy, setBusy] = React.useState(true);
  const [refreshKey, setRefreshKey] = React.useState(0);
  React.useEffect(() => {
    let active = true, inFlight = false;
    async function refresh() {
      if (inFlight) return;
      inFlight = true;
      setBusy(true);
      setError(null);
      try {
        const result = await load();
        if (active) {
          setSnapshots(result.snapshots);
          setDevice(
            (current) => result.snapshots.some((s) => s.device === current) ? current : result.snapshots[0]?.device || ""
          );
        }
      } catch (error2) {
        if (active) setError(String(error2));
      } finally {
        inFlight = false;
        if (active) setBusy(false);
      }
    }
    void refresh();
    const timer = setInterval(() => {
      if (!document.hidden) void refresh();
    }, 6e4);
    return () => {
      active = false;
      clearInterval(timer);
    };
  }, [load, refreshKey]);
  return /* @__PURE__ */ jsxs(Fragment, { children: [
    /* @__PURE__ */ jsxs("div", { className: "fb-heading", children: [
      /* @__PURE__ */ jsx("h1", { children: "Local consumption" }),
      /* @__PURE__ */ jsx("p", { children: "Private usage shared by your Spanreed installations. Devices are shown separately to avoid counting copied logs twice." })
    ] }),
    /* @__PURE__ */ jsx(
      Button,
      {
        variant: "outline",
        className: "",
        disabled: busy,
        onClick: () => setRefreshKey((value) => value + 1),
        children: "Refresh consumption"
      }
    ),
    error && /* @__PURE__ */ jsx("p", { role: "alert", className: "fb-error", children: error }),
    busy && /* @__PURE__ */ jsx("p", { role: "status", children: "Loading consumption…" }),
    !busy && !error && !snapshots.length && /* @__PURE__ */ jsx("p", { children: "No consumption synchronized yet. In Spanreed, enable private synchronization and select the clients you want to share." }),
    snapshots.length > 0 && /* @__PURE__ */ jsxs(Fragment, { children: [
      /* @__PURE__ */ jsxs(Label, { className: "fb-consumption-device", children: [
        "Device",
        " ",
        /* @__PURE__ */ jsx(
          NativeSelect,
          {
            value: device,
            onChange: (event) => setDevice(event.target.value),
            children: [...new Set(snapshots.map((s) => s.device))].map((id) => /* @__PURE__ */ jsx("option", { children: id }, id))
          }
        )
      ] }),
      /* @__PURE__ */ jsx(
        ConsumptionView,
        {
          synchronized: true,
          report: project(snapshots.filter((s) => s.device === device))
        }
      )
    ] })
  ] });
}
export {
  SynchronizedConsumption
};
