import { type ReactNode } from "react";
import { type FaviconResolver } from "./citations";
export declare function ActivityIcon({ icon, live, className }: {
    icon: ReactNode;
    live: boolean;
    className?: string;
}): import("react").JSX.Element;
export type ActivityDisclosureProps = {
    icon: ReactNode;
    live: boolean;
    label: string;
    children?: ReactNode;
    open?: boolean;
    defaultOpen?: boolean;
    onOpenChange?: (open: boolean) => void;
    toggleLabel?: (open: boolean) => string;
    className?: string;
};
export declare function ActivityDisclosure({ icon, live, label, children, open: openProp, defaultOpen, onOpenChange, toggleLabel, className, }: ActivityDisclosureProps): import("react").JSX.Element;
export type ReasoningLabels = {
    thinking?: string;
    brief?: string;
    thoughtFor?: (seconds: number) => string;
};
export declare function reasoningLabel(streaming: boolean, seconds: number | undefined, labels?: ReasoningLabels): string;
export type ReasoningDisclosureProps = {
    streaming?: boolean;
    durationSeconds?: number;
    children?: ReactNode;
    open?: boolean;
    defaultOpen?: boolean;
    onOpenChange?: (open: boolean) => void;
    autoClose?: boolean;
    labels?: ReasoningLabels;
    icon?: ReactNode;
    className?: string;
};
export declare function ReasoningDisclosure({ streaming, durationSeconds, children, open: openProp, defaultOpen, onOpenChange, autoClose, labels, icon, className, }: ReasoningDisclosureProps): import("react").JSX.Element;
export type SearchStep = {
    type: "search";
    query: string;
    sources?: readonly string[];
} | {
    type: "open_page";
    url: string;
    title?: string;
    sources?: readonly string[];
};
export type SearchResultTitle = (url: string) => string | null | undefined;
export type SearchPhase = "searching" | "reading" | "done";
export declare function searchPhase(input: {
    pending: boolean;
    streaming: boolean;
    isLast: boolean;
}): SearchPhase;
export declare function searchDoneLabel(sourceCount: number, searchCount?: number): string;
export declare function searchLabel(phase: SearchPhase, sourceCount: number, searchCount?: number): string;
export declare function countSearches(steps: readonly SearchStep[]): number;
export declare function searchStepLabel(step: SearchStep): string;
export declare function searchStepKindLabel(step: SearchStep): string;
export type SearchStepListProps = {
    steps: readonly SearchStep[];
    live?: boolean;
    titleFor?: SearchResultTitle;
    faviconUrl?: FaviconResolver;
};
export declare function SearchStepList({ steps, live, titleFor, faviconUrl }: SearchStepListProps): import("react").JSX.Element;
export type SearchStepsDisclosureProps = {
    phase: SearchPhase;
    steps?: readonly SearchStep[];
    sourceCount?: number;
    label?: string;
    icon?: ReactNode;
    defaultOpen?: boolean;
    className?: string;
    titleFor?: SearchResultTitle;
    faviconUrl?: FaviconResolver;
};
export declare function SearchStepsDisclosure({ phase, steps, sourceCount, label, icon, defaultOpen, className, titleFor, faviconUrl, }: SearchStepsDisclosureProps): import("react").JSX.Element;
