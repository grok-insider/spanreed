"use client";
import { jsxs, jsx } from "react/jsx-runtime";
import { Card, CardHeader, CardContent, Table } from "@fabrials/ui";
import { useClock, ProviderCard } from "./providers.js";
function SynchronizedAccounts({
  accounts: allAccounts,
  includeLinked = false
}) {
  const accounts = allAccounts.filter(
    (account) => includeLinked || !account.linked_account_id
  );
  const now = useClock();
  if (!accounts.length) return null;
  return /* @__PURE__ */ jsxs(
    "section",
    {
      "aria-label": "Synchronized accounts",
      className: "fb-synchronized-accounts",
      children: [
        !includeLinked && /* @__PURE__ */ jsxs("header", { children: [
          /* @__PURE__ */ jsx("h2", { children: "Accounts from your machines" }),
          /* @__PURE__ */ jsx("p", { className: "fb-muted", children: "Your synchronized usage. Authorize a provider on ai-relay to use it through the hosted proxy." })
        ] }),
        /* @__PURE__ */ jsx(
          "div",
          {
            className: includeLinked ? "fb-synchronized-linked" : "fb-synchronized-grid",
            children: accounts.map((account) => /* @__PURE__ */ jsxs(
              "div",
              {
                className: "fb-synchronized-account",
                children: [
                  /* @__PURE__ */ jsxs("div", { className: "fb-row", children: [
                    /* @__PURE__ */ jsx("strong", { children: account.source }),
                    /* @__PURE__ */ jsx("span", { children: account.linked_account_id ? "Verified identity match" : "Usage synchronized" })
                  ] }),
                  /* @__PURE__ */ jsxs("p", { className: "fb-muted", children: [
                    "Installation ",
                    account.device.slice(0, 8),
                    " ·",
                    " ",
                    now !== null && now - account.observed_at_ms > 9e5 ? "Last known reading · " : "",
                    /* @__PURE__ */ jsx("time", { dateTime: new Date(account.observed_at_ms).toISOString(), children: new Date(account.observed_at_ms).toLocaleString() })
                  ] }),
                  /* @__PURE__ */ jsx(ProviderCard, { provider: account.output }),
                  account.local_usage && /* @__PURE__ */ jsxs(Card, { className: "fb-card-layout", children: [
                    /* @__PURE__ */ jsxs(CardHeader, { children: [
                      /* @__PURE__ */ jsx("h3", { children: "Local log history" }),
                      /* @__PURE__ */ jsx("p", { className: "fb-muted", children: "These logs can contain several accounts and overlap relay requests. Totals are kept separate from relay traffic." })
                    ] }),
                    /* @__PURE__ */ jsxs(CardContent, { className: "fb-content-layout", children: [
                      /* @__PURE__ */ jsxs("p", { children: [
                        account.local_usage.days.reduce((sum, day) => sum + day.tokens, 0).toLocaleString(),
                        " ",
                        "tokens · ~$",
                        account.local_usage.days.reduce((sum, day) => sum + day.estimated_usd, 0).toFixed(2),
                        " ",
                        "API list-price estimate",
                        account.local_usage.partial ? " (partial)" : ""
                      ] }),
                      /* @__PURE__ */ jsxs("details", { children: [
                        /* @__PURE__ */ jsx("summary", { children: "Daily usage" }),
                        /* @__PURE__ */ jsxs(Table, { children: [
                          /* @__PURE__ */ jsx("thead", { children: /* @__PURE__ */ jsxs("tr", { children: [
                            /* @__PURE__ */ jsx("th", { children: "Date" }),
                            /* @__PURE__ */ jsx("th", { children: "Tokens" }),
                            /* @__PURE__ */ jsx("th", { children: "Estimated cost" })
                          ] }) }),
                          /* @__PURE__ */ jsx("tbody", { children: account.local_usage.days.map((day) => /* @__PURE__ */ jsxs("tr", { children: [
                            /* @__PURE__ */ jsx("td", { children: day.date }),
                            /* @__PURE__ */ jsx("td", { children: day.tokens.toLocaleString() }),
                            /* @__PURE__ */ jsxs("td", { children: [
                              "$",
                              day.estimated_usd.toFixed(2)
                            ] })
                          ] }, day.date)) })
                        ] })
                      ] })
                    ] })
                  ] })
                ]
              },
              `${account.device}:${account.source}`
            ))
          }
        )
      ]
    }
  );
}
function LinkedAccountUsage({
  accounts
}) {
  if (!accounts.length) return null;
  return /* @__PURE__ */ jsxs("details", { className: "fb-linked-account-usage", children: [
    /* @__PURE__ */ jsxs("summary", { children: [
      "Usage from ",
      accounts.length,
      " local",
      " ",
      accounts.length === 1 ? "source" : "sources"
    ] }),
    /* @__PURE__ */ jsx(SynchronizedAccounts, { accounts, includeLinked: true })
  ] });
}
export {
  LinkedAccountUsage,
  SynchronizedAccounts
};
