export type VoiceInputStatus = "idle" | "requesting" | "recording" | "transcribing" | "error";
export declare const DEFAULT_MAX_RECORDING_MS = 120000;
export declare const RECORDER_MIME_CANDIDATES: string[];
export declare function pickRecorderMimeType(isTypeSupported: ((type: string) => boolean) | undefined): string | undefined;
export declare function describeMicError(error: unknown): string;
export declare function formatElapsed(ms: number): string;
export declare function insertDictation(value: string, selectionStart: number, selectionEnd: number, text: string): {
    value: string;
    caret: number;
};
export declare function isVoiceInputSupported(): boolean;
export type VoiceInputLabels = {
    idle?: string;
    requesting?: string;
    recording?: string;
    transcribing?: string;
    stopHint?: string;
    empty?: string;
    noSpeech?: string;
    failed?: string;
};
export type VoiceInputButtonViewProps = {
    status: VoiceInputStatus;
    elapsedMs?: number;
    maxDurationMs?: number;
    level?: number;
    error?: string;
    disabled?: boolean;
    labels?: VoiceInputLabels;
    onPress?: () => void;
    className?: string;
};
export declare function VoiceInputButtonView({ status, elapsedMs, maxDurationMs, level, error, disabled, labels: labelOverrides, onPress, className, }: VoiceInputButtonViewProps): import("react").JSX.Element;
export type VoiceInputButtonProps = {
    onRecorded: (blob: Blob) => Promise<string>;
    onTranscript: (text: string) => void;
    onError?: (message: string, error?: unknown) => void;
    onNotice?: (message: string) => void;
    maxDurationMs?: number;
    showLevel?: boolean;
    disabled?: boolean;
    labels?: VoiceInputLabels;
    className?: string;
};
export declare function VoiceInputButton({ onRecorded, onTranscript, onError, onNotice, maxDurationMs, showLevel, disabled, labels: labelOverrides, className, }: VoiceInputButtonProps): import("react").JSX.Element | null;
