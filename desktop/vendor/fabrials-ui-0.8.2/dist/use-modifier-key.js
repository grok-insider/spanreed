"use client";
import { jsx, Fragment } from "react/jsx-runtime";
import { useSyncExternalStore } from "react";
const noSubscription = () => () => {
};
function isApplePlatform(platform) {
  return /mac|iphone|ipad|ipod/i.test(platform);
}
function onApplePlatform() {
  const nav = navigator;
  return isApplePlatform(nav.userAgentData?.platform || nav.platform || "");
}
function useModifierKey() {
  return useSyncExternalStore(noSubscription, () => onApplePlatform() ? "⌘" : "Ctrl", () => "Ctrl");
}
function ModifierKeyText() {
  return /* @__PURE__ */ jsx(Fragment, { children: useModifierKey() });
}
export {
  ModifierKeyText,
  isApplePlatform,
  useModifierKey
};
