"use client";
import { jsxs, Fragment, jsx } from "react/jsx-runtime";
import { Card, CardHeader, CardContent, Label, NativeSelect, Table } from "@fabrials/ui";
import * as React from "react";
const number = new Intl.NumberFormat();
const money = new Intl.NumberFormat(void 0, {
  style: "currency",
  currency: "USD",
  maximumFractionDigits: 2
});
function ConsumptionView({
  report,
  synchronized = false
}) {
  const [group, setGroup] = React.useState("daily");
  return /* @__PURE__ */ jsxs(Fragment, { children: [
    /* @__PURE__ */ jsxs("div", { className: "fb-grid", children: [
      /* @__PURE__ */ jsxs(Card, { className: "fb-card-layout", children: [
        /* @__PURE__ */ jsx(CardHeader, { children: /* @__PURE__ */ jsx("h2", { children: "Tokens" }) }),
        /* @__PURE__ */ jsxs(CardContent, { className: "fb-content-layout", children: [
          /* @__PURE__ */ jsx("strong", { children: number.format(report.total.tokens) }),
          /* @__PURE__ */ jsxs("p", { className: "fb-muted", children: [
            "Recorded by your clients",
            report.total.unknown_token_records > 0 && " · token counts incomplete"
          ] })
        ] })
      ] }),
      /* @__PURE__ */ jsxs(Card, { className: "fb-card-layout", children: [
        /* @__PURE__ */ jsx(CardHeader, { children: /* @__PURE__ */ jsx("h2", { children: "Known cost" }) }),
        /* @__PURE__ */ jsxs(CardContent, { className: "fb-content-layout", children: [
          /* @__PURE__ */ jsx("strong", { children: money.format(report.total.known_usd) }),
          /* @__PURE__ */ jsx("p", { className: "fb-muted", children: report.total.partial ? "Partial · some data or prices are unavailable" : "Reported costs and list-price estimates" })
        ] })
      ] }),
      /* @__PURE__ */ jsxs(Card, { className: "fb-card-layout", children: [
        /* @__PURE__ */ jsx(CardHeader, { children: /* @__PURE__ */ jsx("h2", { children: "Sources with data" }) }),
        /* @__PURE__ */ jsxs(CardContent, { className: "fb-content-layout", children: [
          /* @__PURE__ */ jsx("strong", { children: report.sources.filter((source) => source.records > 0).length }),
          /* @__PURE__ */ jsx("p", { className: "fb-muted", children: "Local consumption, separate from proxy traffic and quotas" })
        ] })
      ] })
    ] }),
    /* @__PURE__ */ jsxs(Card, { className: "fb-card-layout", children: [
      /* @__PURE__ */ jsxs(CardHeader, { className: "fb-row fb-consumption-filters", children: [
        /* @__PURE__ */ jsx("h2", { children: "Consumption breakdown" }),
        /* @__PURE__ */ jsxs(Label, { children: [
          "Group by",
          " ",
          /* @__PURE__ */ jsxs(
            NativeSelect,
            {
              value: group,
              onChange: (event) => setGroup(event.target.value),
              children: [
                /* @__PURE__ */ jsx("option", { value: "daily", children: "Day" }),
                /* @__PURE__ */ jsx("option", { value: "period_totals", children: "Session / account totals" }),
                /* @__PURE__ */ jsx("option", { value: "clients", children: "Client" }),
                !synchronized && /* @__PURE__ */ jsxs(Fragment, { children: [
                  /* @__PURE__ */ jsx("option", { value: "models", children: "Model" }),
                  /* @__PURE__ */ jsx("option", { value: "sessions", children: "Session" }),
                  /* @__PURE__ */ jsx("option", { value: "projects", children: "Project" })
                ] })
              ]
            }
          )
        ] })
      ] }),
      group === "daily" && report.period_totals.length > 0 && /* @__PURE__ */ jsx("p", { className: "fb-content-layout", children: "Some sources report session or account totals. See “Session / account totals”; these are not split into estimated daily activity." }),
      /* @__PURE__ */ jsx("div", { className: "fb-table-wrap", children: /* @__PURE__ */ jsxs(Table, { className: "fb-table-layout", children: [
        /* @__PURE__ */ jsx("thead", { children: /* @__PURE__ */ jsxs("tr", { children: [
          /* @__PURE__ */ jsx("th", { scope: "col", children: group }),
          /* @__PURE__ */ jsx("th", { scope: "col", children: "Tokens" }),
          /* @__PURE__ */ jsx("th", { scope: "col", children: "Known cost" })
        ] }) }),
        /* @__PURE__ */ jsx("tbody", { children: report[group].map((row) => /* @__PURE__ */ jsxs("tr", { children: [
          /* @__PURE__ */ jsx("th", { scope: "row", style: { overflowWrap: "anywhere" }, children: row.key }),
          /* @__PURE__ */ jsx("td", { children: number.format(row.tokens) }),
          /* @__PURE__ */ jsxs("td", { children: [
            money.format(row.known_usd),
            row.partial ? " · partial" : ""
          ] })
        ] }, row.key)) })
      ] }) }),
      !report[group].length && /* @__PURE__ */ jsx("p", { className: "fb-content-layout", children: "No recorded usage in this period." })
    ] }),
    /* @__PURE__ */ jsxs("details", { className: "fb-card-layout", children: [
      /* @__PURE__ */ jsx("summary", { className: "fb-content-layout", children: "Source status" }),
      /* @__PURE__ */ jsx(CardContent, { className: "fb-content-layout", children: report.sources.map((source) => /* @__PURE__ */ jsxs("p", { children: [
        /* @__PURE__ */ jsx("strong", { children: source.client }),
        " ·",
        " ",
        source.state.replaceAll("_", " "),
        source.detail && /* @__PURE__ */ jsxs(Fragment, { children: [
          " — ",
          source.detail
        ] })
      ] }, source.client)) })
    ] })
  ] });
}
export {
  ConsumptionView
};
