"use client";
import { jsxs, jsx } from "react/jsx-runtime";
import { Label, NativeSelect, Input, Card, CardHeader, CardContent, Button } from "@fabrials/ui";
import * as React from "react";
function ApiKeyFields({
  provider,
  alias,
  secret,
  onProviderChange,
  onAliasChange,
  onSecretChange,
  pending = false,
  lockedIdentity = false,
  providers = [
    { id: "openai", label: "OpenAI API" },
    { id: "nous", label: "Nous API" }
  ]
}) {
  return /* @__PURE__ */ jsxs("div", { className: "fb-form", children: [
    /* @__PURE__ */ jsxs(Label, { children: [
      "Provider",
      /* @__PURE__ */ jsx(
        NativeSelect,
        {
          name: "provider",
          value: provider,
          disabled: pending || lockedIdentity,
          onChange: (event) => onProviderChange(event.target.value),
          children: providers.map((option) => /* @__PURE__ */ jsx("option", { value: option.id, children: option.label }, option.id))
        }
      )
    ] }),
    /* @__PURE__ */ jsxs(Label, { children: [
      "Account name",
      /* @__PURE__ */ jsx(
        Input,
        {
          name: "alias",
          required: true,
          pattern: "[A-Za-z0-9_-]+",
          maxLength: 40,
          autoComplete: "off",
          value: alias,
          disabled: pending || lockedIdentity,
          onChange: (event) => onAliasChange(event.target.value)
        }
      )
    ] }),
    /* @__PURE__ */ jsxs(Label, { children: [
      "API key",
      /* @__PURE__ */ jsx(
        Input,
        {
          name: "api_key",
          type: "password",
          required: true,
          maxLength: 8192,
          autoComplete: "off",
          autoCapitalize: "none",
          spellCheck: false,
          value: secret,
          disabled: pending,
          onChange: onSecretChange ? (event) => onSecretChange(event.target.value) : void 0
        }
      )
    ] })
  ] });
}
function ApiKeyForm({
  onSave,
  accounts
}) {
  const [provider, setProvider] = React.useState("openai");
  const [alias, setAlias] = React.useState("");
  const [key, setKey] = React.useState("");
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState(null);
  const [saved, setSaved] = React.useState(null);
  const showSaved = saved && (!accounts || accounts.some(
    (account) => account.provider === saved.provider && account.alias === saved.alias
  ));
  async function save(event) {
    event.preventDefault();
    if (busy) return;
    setBusy(true);
    setError(null);
    setSaved(null);
    try {
      await onSave({ provider, alias: alias.trim(), key: key.trim() });
      setKey("");
      setSaved({ provider, alias: alias.trim() });
    } catch (error2) {
      setError(String(error2));
    } finally {
      setBusy(false);
    }
  }
  return /* @__PURE__ */ jsxs(Card, { className: "fb-card-layout", children: [
    /* @__PURE__ */ jsxs(CardHeader, { children: [
      /* @__PURE__ */ jsx("h2", { children: "Connect with an API key" }),
      /* @__PURE__ */ jsx("p", { className: "fb-muted", children: "Use an existing provider key. Requests use that provider’s API billing." })
    ] }),
    /* @__PURE__ */ jsx(CardContent, { className: "fb-content-layout", children: /* @__PURE__ */ jsxs("form", { className: "fb-form", onSubmit: (event) => void save(event), children: [
      /* @__PURE__ */ jsx(
        ApiKeyFields,
        {
          provider,
          alias,
          secret: key,
          pending: busy,
          onProviderChange: (value) => {
            setProvider(value);
            setKey("");
            setSaved(null);
          },
          onAliasChange: (value) => {
            setAlias(value);
            setSaved(null);
          },
          onSecretChange: (value) => {
            setKey(value);
            setSaved(null);
          }
        }
      ),
      /* @__PURE__ */ jsx(Button, { className: " ", type: "submit", disabled: busy, children: busy ? "Saving…" : "Save API key" }),
      error && /* @__PURE__ */ jsx("p", { className: "fb-error", role: "alert", children: error }),
      showSaved && /* @__PURE__ */ jsxs("p", { role: "status", children: [
        "Saved ",
        saved.provider,
        "/",
        saved.alias,
        ". The key has not been verified with the provider."
      ] })
    ] }) })
  ] });
}
export {
  ApiKeyFields,
  ApiKeyForm
};
