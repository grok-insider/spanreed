import type { ComponentProps, HTMLAttributes, ReactNode } from "react";
import { Button } from "@fabrials/ui";
export type ChatRole = "user" | "assistant" | "system";
export type ChatMessageProps = HTMLAttributes<HTMLDivElement> & {
    from: ChatRole;
    actions?: ReactNode;
    pinActions?: boolean;
    bodyClassName?: string;
};
export declare function ChatMessage({ from, actions, pinActions, className, bodyClassName, children, ...props }: ChatMessageProps): import("react").JSX.Element;
export declare function MessageActions({ className, ...props }: ComponentProps<"div">): import("react").JSX.Element;
export type MessageActionProps = Omit<ComponentProps<typeof Button>, "children"> & {
    label: string;
    tooltip?: ReactNode;
    children: ReactNode;
};
export declare function MessageAction({ label, tooltip, children, className, ...props }: MessageActionProps): import("react").JSX.Element;
export declare function CopyMessageAction({ text, onCopy, label, copiedLabel, ...props }: Omit<MessageActionProps, "label" | "children" | "onClick" | "onCopy"> & {
    text: string | (() => string);
    onCopy?: (text: string) => void | Promise<void>;
    label?: string;
    copiedLabel?: string;
}): import("react").JSX.Element;
export declare function MessageTimestamp({ dateTime, label, detail, className, }: {
    dateTime: string;
    label: string;
    detail?: ReactNode;
    className?: string;
}): import("react").JSX.Element;
