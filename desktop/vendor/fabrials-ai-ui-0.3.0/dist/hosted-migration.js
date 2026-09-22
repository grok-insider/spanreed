"use client";
import { jsxs, jsx, Fragment } from "react/jsx-runtime";
import { PageHeader, Label, NativeSelect, Button, Input, NativeCheckbox } from "@fabrials/ui";
import * as React from "react";
import { MigrationReviewDetails } from "./migration-review.js";
function HostedMigration({
  api: migrationApi,
  origin,
  authorize
}) {
  const [authorized, setAuthorized] = React.useState([]);
  const [direction, setDirection] = React.useState("hostedToLocal");
  const [invitation, setInvitation] = React.useState(null);
  const [sessions, setSessions] = React.useState([]);
  const [selected, setSelected] = React.useState(
    null
  );
  const [forgetting, setForgetting] = React.useState(false);
  const [confirmed, setConfirmed] = React.useState(false);
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState(null);
  const errorRef = React.useRef(null);
  React.useEffect(() => {
    if (error) errorRef.current?.focus();
  }, [error]);
  const [now, setNow] = React.useState(() => Date.now());
  async function refresh() {
    const data = await migrationApi.sessions();
    setSessions(data.sessions);
  }
  React.useEffect(() => {
    const controller = new AbortController();
    migrationApi.sessions().then((data) => setSessions(data.sessions)).catch((error2) => {
      if (!controller.signal.aborted) setError(String(error2));
    });
    const timer = setInterval(() => setNow(Date.now()), 1e3);
    return () => {
      controller.abort();
      clearInterval(timer);
    };
  }, [migrationApi]);
  async function run(action) {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      await action();
    } catch (error2) {
      setError(String(error2));
    } finally {
      setBusy(false);
    }
  }
  async function open(id) {
    const view = await migrationApi.status({ id });
    setSessions(
      (current) => current.map((session) => session.id === view.id ? view : session)
    );
    setAuthorized([]);
    setSelected(view);
    setConfirmed(false);
    setForgetting(false);
    if (view.phase === "completed" && view.direction === "localToHosted")
      setAuthorized(await migrationApi.authorizations({ id }));
    if (view.phase !== "pairing") setInvitation(null);
  }
  return /* @__PURE__ */ jsxs("div", { className: "fb-form", children: [
    /* @__PURE__ */ jsx(PageHeader, { title: "Account migration", description: "Pair Spanreed with this hosted workspace. Review the destination and selected accounts before approving a transfer." }),
    /* @__PURE__ */ jsxs("section", { className: "fb-form", "aria-label": "Create migration invitation", children: [
      /* @__PURE__ */ jsxs(Label, { children: [
        "Direction",
        /* @__PURE__ */ jsxs(
          NativeSelect,
          {
            value: direction,
            disabled: busy,
            onChange: (event) => setDirection(event.target.value),
            children: [
              /* @__PURE__ */ jsx("option", { value: "hostedToLocal", children: "ai-relay → Spanreed" }),
              /* @__PURE__ */ jsx("option", { value: "localToHosted", children: "Spanreed → ai-relay" })
            ]
          }
        )
      ] }),
      /* @__PURE__ */ jsx(
        Button,
        {
          variant: "outline",
          className: "",
          disabled: busy,
          onClick: () => void run(async () => {
            const result = await migrationApi.create({ direction });
            setInvitation(result);
            setSelected(null);
            setConfirmed(false);
            await refresh();
          }),
          children: "Create invitation"
        }
      ),
      invitation && /* @__PURE__ */ jsxs("div", { className: "fb-form", children: [
        /* @__PURE__ */ jsxs("p", { children: [
          "Enter these details in Spanreed → Migration. The invitation expires ",
          new Date(invitation.expiresAtMs).toLocaleTimeString(),
          "."
        ] }),
        /* @__PURE__ */ jsxs(Label, { children: [
          "Hosted origin",
          /* @__PURE__ */ jsx(Input, { readOnly: true, value: origin })
        ] }),
        /* @__PURE__ */ jsxs(Label, { children: [
          "Session ID",
          /* @__PURE__ */ jsx(Input, { readOnly: true, value: invitation.id })
        ] }),
        now < invitation.expiresAtMs ? /* @__PURE__ */ jsxs(Label, { children: [
          "Invitation secret",
          /* @__PURE__ */ jsx(
            Input,
            {
              readOnly: true,
              type: "password",
              autoComplete: "off",
              value: invitation.secret,
              onFocus: (event) => event.currentTarget.select()
            }
          )
        ] }) : /* @__PURE__ */ jsx("p", { role: "alert", children: "Invitation expired. Create another invitation." }),
        now < invitation.expiresAtMs && /* @__PURE__ */ jsx(
          Button,
          {
            variant: "outline",
            className: "",
            disabled: busy,
            onClick: () => void run(async () => {
              await navigator.clipboard.writeText(invitation.secret);
            }),
            children: "Copy invitation secret"
          }
        ),
        /* @__PURE__ */ jsx("p", { className: "fb-muted", children: "The secret is shown only in this page session. Copy it into Spanreed before closing this page." }),
        /* @__PURE__ */ jsx(
          Button,
          {
            variant: "outline",
            className: "",
            disabled: busy,
            onClick: () => void run(async () => open(invitation.id)),
            children: "Check pairing"
          }
        ),
        /* @__PURE__ */ jsx(
          Button,
          {
            variant: "outline",
            className: "",
            onClick: () => setInvitation(null),
            children: "Hide invitation"
          }
        )
      ] })
    ] }),
    /* @__PURE__ */ jsxs("section", { className: "fb-form", "aria-label": "Migration sessions", children: [
      /* @__PURE__ */ jsx("h2", { children: "Sessions" }),
      /* @__PURE__ */ jsx("p", { className: "fb-muted", children: "Completed receipts remain available for 30 days after the transfer window closes." }),
      /* @__PURE__ */ jsx(
        Button,
        {
          variant: "outline",
          className: "",
          disabled: busy,
          onClick: () => void run(refresh),
          children: "Refresh sessions"
        }
      ),
      !sessions.length && /* @__PURE__ */ jsx("p", { className: "fb-muted", children: "No migration sessions." }),
      sessions.map((session) => /* @__PURE__ */ jsx(
        Button,
        {
          variant: "outline",
          className: "",
          disabled: busy,
          onClick: () => void run(async () => open(session.id)),
          children: /* @__PURE__ */ jsxs("span", { style: { overflowWrap: "anywhere" }, children: [
            session.direction === "hostedToLocal" ? "ai-relay → Spanreed" : "Spanreed → ai-relay",
            " ",
            "· ",
            session.phase,
            /* @__PURE__ */ jsx("br", {}),
            session.id
          ] })
        },
        session.id
      ))
    ] }),
    selected && /* @__PURE__ */ jsxs("section", { className: "fb-form", "aria-label": "Selected migration", children: [
      /* @__PURE__ */ jsx("h2", { children: "Review transfer" }),
      /* @__PURE__ */ jsxs("p", { style: { overflowWrap: "anywhere" }, children: [
        "Session: ",
        /* @__PURE__ */ jsx("code", { children: selected.id }),
        /* @__PURE__ */ jsx("br", {}),
        "Local installation:",
        " ",
        /* @__PURE__ */ jsx("code", { children: selected.localEnvironment || "Waiting for pairing" }),
        /* @__PURE__ */ jsx("br", {}),
        "Status: ",
        selected.phase
      ] }),
      selected.review && /* @__PURE__ */ jsx(MigrationReviewDetails, { review: selected.review }),
      selected.phase === "paired" && /* @__PURE__ */ jsx("p", { children: "Select accounts and create a review in Spanreed, then refresh this session." }),
      selected.phase === "reviewing" && /* @__PURE__ */ jsxs(Fragment, { children: [
        /* @__PURE__ */ jsxs(Label, { children: [
          /* @__PURE__ */ jsx(
            NativeCheckbox,
            {
              type: "checkbox",
              checked: confirmed,
              disabled: busy || now >= selected.expiresAtMs,
              onChange: (event) => setConfirmed(event.target.checked)
            }
          ),
          " ",
          "I recognize this installation and approve the exact accounts and destination above."
        ] }),
        /* @__PURE__ */ jsx(
          Button,
          {
            className: " ",
            disabled: busy || !confirmed || !selected.revision || now >= selected.expiresAtMs,
            onClick: () => void run(async () => {
              if (!selected.revision)
                throw new Error(
                  "Review unavailable. Refresh this session."
                );
              await migrationApi.approve({
                id: selected.id,
                revision: selected.revision
              });
              await open(selected.id);
              await refresh();
            }),
            children: "Approve review"
          }
        )
      ] }),
      selected.phase === "approved" && /* @__PURE__ */ jsx("p", { role: "status", children: "Approved. Confirm and execute the transfer in Spanreed." }),
      selected.phase === "completed" && /* @__PURE__ */ jsx("p", { role: "status", children: "API-key transfer completed. OAuth entries require independent authorization in the destination." }),
      selected.phase === "completed" && selected.direction === "localToHosted" && selected.review?.items.filter(
        (item) => item.action === "authorizeOAuth" && !authorized.includes(`${item.provider}/${item.targetAlias}`)
      ).map(
        (item) => (item.provider === "grok" || item.provider === "nous" || item.provider === "codex") && /* @__PURE__ */ jsx("div", { children: authorize({
          provider: item.provider,
          alias: item.targetAlias,
          migrationId: selected.id,
          sourceId: item.sourceId,
          onConnected: async () => {
            setAuthorized(
              await migrationApi.authorizations({
                id: selected.id
              })
            );
          }
        }) }, `${selected.id}/${item.sourceId}`)
      ),
      authorized.map((id) => /* @__PURE__ */ jsxs("p", { role: "status", children: [
        "Connected ",
        id,
        " with a new hosted authorization."
      ] }, id)),
      now >= selected.expiresAtMs && selected.phase !== "completed" && /* @__PURE__ */ jsx("p", { role: "alert", children: "This session expired." }),
      selected.phase === "completed" && /* @__PURE__ */ jsx("div", { className: "fb-form", children: !forgetting ? /* @__PURE__ */ jsx(
        Button,
        {
          variant: "outline",
          className: "",
          disabled: busy,
          onClick: () => setForgetting(true),
          children: "Forget receipt"
        }
      ) : /* @__PURE__ */ jsxs(Fragment, { children: [
        /* @__PURE__ */ jsx("p", { children: "Remove this migration receipt from ai-relay? Imported accounts remain connected. The reviewed aliases will no longer be available here for pending OAuth logins." }),
        /* @__PURE__ */ jsxs("div", { className: "fb-row", children: [
          /* @__PURE__ */ jsx(
            Button,
            {
              variant: "outline",
              className: "",
              disabled: busy,
              onClick: () => void run(async () => {
                await migrationApi.forget({ id: selected.id });
                setSelected(null);
                setForgetting(false);
                await refresh();
              }),
              children: "Forget this receipt"
            }
          ),
          /* @__PURE__ */ jsx(
            Button,
            {
              variant: "outline",
              className: "",
              disabled: busy,
              onClick: () => setForgetting(false),
              children: "Keep receipt"
            }
          )
        ] })
      ] }) }),
      /* @__PURE__ */ jsxs("div", { className: "fb-row", children: [
        /* @__PURE__ */ jsx(
          Button,
          {
            variant: "outline",
            className: "",
            disabled: busy,
            onClick: () => void run(async () => open(selected.id)),
            children: "Refresh selected session"
          }
        ),
        selected.phase !== "completed" && selected.phase !== "cancelled" && /* @__PURE__ */ jsx(
          Button,
          {
            variant: "outline",
            className: "",
            disabled: busy || now >= selected.expiresAtMs,
            onClick: () => void run(async () => {
              await migrationApi.cancel({ id: selected.id });
              setInvitation(null);
              setSelected(null);
              setConfirmed(false);
              await refresh();
            }),
            children: "Cancel migration"
          }
        )
      ] })
    ] }),
    busy && /* @__PURE__ */ jsx("p", { role: "status", children: "Updating migration…" }),
    error && /* @__PURE__ */ jsx("p", { ref: errorRef, tabIndex: -1, className: "fb-error", role: "alert", children: error })
  ] });
}
export {
  HostedMigration
};
