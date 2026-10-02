import type { ReactNode } from "react";
export type AttachmentItem = {
    id: string;
    name: string;
    mediaType: string;
    url?: string;
    size?: number;
    status?: "ready" | "uploading" | "error";
    error?: string;
};
export type AttachmentCategory = "image" | "video" | "audio" | "document" | "unknown";
export type AttachmentVariant = "grid" | "inline" | "list";
export declare function attachmentCategory(mediaType: string | undefined): AttachmentCategory;
export declare function formatBytes(bytes: number | undefined): string;
export declare function attachmentDetail(item: AttachmentItem): string;
export type AttachmentChipProps = {
    item: AttachmentItem;
    variant?: AttachmentVariant;
    onRemove?: (id: string) => void;
    removeLabel?: (item: AttachmentItem) => string;
    className?: string;
};
export declare function AttachmentChip({ item, variant, onRemove, removeLabel, className, }: AttachmentChipProps): import("react").JSX.Element;
export type AttachmentsProps = {
    items: readonly AttachmentItem[];
    variant?: AttachmentVariant;
    onRemove?: (id: string) => void;
    removeLabel?: (item: AttachmentItem) => string;
    empty?: ReactNode;
    label?: string;
    className?: string;
};
export declare function Attachments({ items, variant, onRemove, removeLabel, empty, label, className, }: AttachmentsProps): import("react").JSX.Element | null;
