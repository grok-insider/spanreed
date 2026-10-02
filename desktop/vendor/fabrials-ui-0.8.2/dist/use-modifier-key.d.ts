export type ModifierKey = "Ctrl" | "⌘";
/**
 * Whether a platform string names an Apple device. Case-insensitive on purpose: `navigator.platform` says "MacIntel" but
 * Chromium's `userAgentData.platform` says "macOS", which a case-sensitive `/Mac/` does not match (the hint stayed «Ctrl»
 * on Chrome and Edge for Mac).
 */
export declare function isApplePlatform(platform: string): boolean;
/**
 * The name of the main modifier key as this device prints it. It is "Ctrl" on the server and on the first client render
 * and becomes "⌘" on Apple platforms right after hydration, so the markup never mismatches. Use it for the label of a
 * shortcut; the shortcut itself should test `event.ctrlKey || event.metaKey`.
 */
export declare function useModifierKey(): ModifierKey;
/** The modifier as text: what `<Kbd mod />` renders. */
export declare function ModifierKeyText(): import("react").JSX.Element;
