"use client";
import { jsx, jsxs, Fragment } from "react/jsx-runtime";
import { useSyncExternalStore, useState, useRef, useEffect, useCallback } from "react";
import { Spinner, Tooltip, TooltipTrigger, TooltipContent } from "@fabrials/ui";
import { MicIcon } from "./chat-icons.js";
const DEFAULT_MAX_RECORDING_MS = 12e4;
const RECORDER_MIME_CANDIDATES = [
  "audio/webm;codecs=opus",
  "audio/webm",
  "audio/mp4",
  "audio/ogg;codecs=opus"
];
function pickRecorderMimeType(isTypeSupported) {
  if (!isTypeSupported) return void 0;
  return RECORDER_MIME_CANDIDATES.find((type) => {
    try {
      return isTypeSupported(type);
    } catch {
      return false;
    }
  });
}
function describeMicError(error) {
  const name = error instanceof Error || typeof error === "object" && error && "name" in error ? String(error.name) : "";
  switch (name) {
    case "NotAllowedError":
    case "PermissionDeniedError":
    case "SecurityError":
      return "Microphone access is blocked. Allow it in your browser's site settings to dictate.";
    case "NotFoundError":
    case "DevicesNotFoundError":
    case "OverconstrainedError":
      return "No microphone was found.";
    case "NotReadableError":
    case "TrackStartError":
      return "The microphone is busy in another app.";
    case "AbortError":
      return "Recording was interrupted.";
    default:
      return "Couldn't start the microphone.";
  }
}
function formatElapsed(ms) {
  const total = Math.max(0, Math.floor(ms / 1e3));
  return `${Math.floor(total / 60)}:${(total % 60).toString().padStart(2, "0")}`;
}
function insertDictation(value, selectionStart, selectionEnd, text) {
  const trimmed = text.trim();
  const start = Math.max(0, Math.min(selectionStart, selectionEnd, value.length));
  const end = Math.max(start, Math.min(Math.max(selectionStart, selectionEnd), value.length));
  if (!trimmed) return { value, caret: end };
  const before = value.slice(0, start);
  const after = value.slice(end);
  const lead = before && !/\s$/.test(before) ? " " : "";
  const trail = after && !/^\s/.test(after) ? " " : "";
  const insert = `${lead}${trimmed}`;
  return { value: `${before}${insert}${trail}${after}`, caret: before.length + insert.length };
}
function isVoiceInputSupported() {
  return typeof window !== "undefined" && typeof MediaRecorder !== "undefined" && typeof navigator !== "undefined" && typeof navigator.mediaDevices?.getUserMedia === "function";
}
const DEFAULT_LABELS = {
  idle: "Dictate",
  requesting: "Waiting for microphone",
  recording: "Stop recording",
  transcribing: "Transcribing",
  stopHint: "Stop and transcribe",
  empty: "Nothing was recorded.",
  noSpeech: "Didn't catch that — try again a little closer to the mic.",
  failed: "Transcription failed."
};
function VoiceInputButtonView({
  status,
  elapsedMs = 0,
  maxDurationMs,
  level,
  error,
  disabled = false,
  labels: labelOverrides,
  onPress,
  className
}) {
  const labels = { ...DEFAULT_LABELS, ...labelOverrides };
  const recording = status === "recording";
  const busy = status === "transcribing" || status === "requesting";
  const label = recording ? labels.recording : status === "transcribing" ? labels.transcribing : status === "requesting" ? labels.requesting : labels.idle;
  const remaining = maxDurationMs ? Math.max(0, maxDurationMs - elapsedMs) : void 0;
  const hint = status === "error" && error ? error : recording ? labels.stopHint : label;
  const button = /* @__PURE__ */ jsxs(
    "button",
    {
      "aria-busy": busy || void 0,
      "aria-disabled": busy || disabled || void 0,
      "aria-label": label,
      "aria-pressed": recording,
      className: ["fui-voice-button", className].filter(Boolean).join(" "),
      "data-status": status,
      onClick: () => {
        if (busy || disabled) return;
        onPress?.();
      },
      style: level === void 0 ? void 0 : { "--fui-voice-level": Math.max(0, Math.min(1, level)).toFixed(3) },
      type: "button",
      children: [
        status === "transcribing" || status === "requesting" ? /* @__PURE__ */ jsx(Spinner, { "aria-hidden": true, label: "", role: "presentation" }) : /* @__PURE__ */ jsxs("span", { className: "fui-voice-icon", children: [
          /* @__PURE__ */ jsx(MicIcon, {}),
          /* @__PURE__ */ jsx("span", { "aria-hidden": true, className: "fui-voice-dot" })
        ] }),
        recording ? /* @__PURE__ */ jsx("span", { className: "fui-voice-timer", title: remaining !== void 0 ? `${formatElapsed(remaining)} left` : void 0, children: formatElapsed(elapsedMs) }) : null,
        status === "transcribing" ? /* @__PURE__ */ jsxs("span", { className: "fui-voice-text", children: [
          labels.transcribing,
          "…"
        ] }) : null
      ]
    }
  );
  return /* @__PURE__ */ jsxs(Fragment, { children: [
    /* @__PURE__ */ jsxs(Tooltip, { children: [
      /* @__PURE__ */ jsx(TooltipTrigger, { render: button }),
      /* @__PURE__ */ jsx(TooltipContent, { children: hint })
    ] }),
    /* @__PURE__ */ jsx("span", { "aria-live": "polite", className: "fui-sr-only", role: "status", children: status === "error" && error ? error : "" })
  ] });
}
const noopSubscribe = () => () => {
};
function stopTracks(stream) {
  for (const track of stream?.getTracks() ?? []) track.stop();
}
function VoiceInputButton({
  onRecorded,
  onTranscript,
  onError,
  onNotice,
  maxDurationMs = DEFAULT_MAX_RECORDING_MS,
  showLevel = false,
  disabled,
  labels: labelOverrides,
  className
}) {
  const supported = useSyncExternalStore(noopSubscribe, isVoiceInputSupported, () => false);
  const labels = { ...DEFAULT_LABELS, ...labelOverrides };
  const [status, setStatus] = useState("idle");
  const [error, setError] = useState();
  const [elapsedMs, setElapsedMs] = useState(0);
  const [level, setLevel] = useState(void 0);
  const recorderRef = useRef(null);
  const streamRef = useRef(null);
  const chunksRef = useRef([]);
  const audioRef = useRef({});
  const timersRef = useRef({});
  const mountedRef = useRef(true);
  const callbacks = useRef({ onRecorded, onTranscript, onError, onNotice, labels });
  useEffect(() => {
    callbacks.current = { onRecorded, onTranscript, onError, onNotice, labels };
  });
  const teardown = useCallback(() => {
    clearInterval(timersRef.current.tick);
    clearTimeout(timersRef.current.stop);
    timersRef.current = {};
    if (audioRef.current.frame !== void 0) cancelAnimationFrame(audioRef.current.frame);
    void audioRef.current.context?.close().catch(() => void 0);
    audioRef.current = {};
    stopTracks(streamRef.current);
    streamRef.current = null;
    recorderRef.current = null;
  }, []);
  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      const recorder = recorderRef.current;
      if (recorder && recorder.state !== "inactive") {
        recorder.onstop = null;
        recorder.stop();
      }
      teardown();
    };
  }, [teardown]);
  const fail = useCallback((message, cause) => {
    setError(message);
    setStatus("error");
    callbacks.current.onError?.(message, cause);
  }, []);
  const stop = useCallback(() => {
    const recorder = recorderRef.current;
    if (recorder && recorder.state !== "inactive") recorder.stop();
  }, []);
  const transcribe = useCallback(
    async (blob) => {
      const { onRecorded: record, onTranscript: deliver, onNotice: notice, labels: text } = callbacks.current;
      if (blob.size === 0) {
        notice?.(text.empty);
        setStatus("idle");
        return;
      }
      setStatus("transcribing");
      try {
        const transcript = (await record(blob)).trim();
        if (!mountedRef.current) return;
        if (!transcript) notice?.(text.noSpeech);
        else deliver(transcript);
        setStatus("idle");
      } catch (cause) {
        if (!mountedRef.current) return;
        fail(cause instanceof Error && cause.message ? cause.message : text.failed, cause);
      }
    },
    [fail]
  );
  const meter = useCallback((stream) => {
    const Context = window.AudioContext ?? window.webkitAudioContext;
    if (!Context) return;
    try {
      const context = new Context();
      const analyser = context.createAnalyser();
      analyser.fftSize = 256;
      context.createMediaStreamSource(stream).connect(analyser);
      const data = new Uint8Array(analyser.fftSize);
      const read = () => {
        analyser.getByteTimeDomainData(data);
        let sum = 0;
        for (const sample of data) sum += ((sample - 128) / 128) ** 2;
        setLevel(Math.round(Math.min(1, Math.sqrt(sum / data.length) * 3) * 20) / 20);
        audioRef.current.frame = requestAnimationFrame(read);
      };
      audioRef.current = { context, frame: requestAnimationFrame(read) };
    } catch {
      setLevel(void 0);
    }
  }, []);
  const start = useCallback(async () => {
    if (recorderRef.current) return;
    setError(void 0);
    setStatus("requesting");
    let stream;
    try {
      stream = await navigator.mediaDevices.getUserMedia({ audio: true });
    } catch (cause) {
      if (mountedRef.current) fail(describeMicError(cause), cause);
      return;
    }
    if (!mountedRef.current) {
      stopTracks(stream);
      return;
    }
    const mimeType = pickRecorderMimeType(
      typeof MediaRecorder.isTypeSupported === "function" ? (type) => MediaRecorder.isTypeSupported(type) : void 0
    );
    let recorder;
    try {
      recorder = new MediaRecorder(stream, mimeType ? { mimeType } : void 0);
    } catch (cause) {
      stopTracks(stream);
      fail(describeMicError(cause), cause);
      return;
    }
    chunksRef.current = [];
    recorder.ondataavailable = (event) => {
      if (event.data.size > 0) chunksRef.current.push(event.data);
    };
    recorder.onerror = (event) => {
      recorder.onstop = null;
      teardown();
      if (mountedRef.current) fail("Recording failed.", event);
    };
    recorder.onstop = () => {
      const blob = new Blob(chunksRef.current, { type: recorder.mimeType || mimeType || "audio/webm" });
      chunksRef.current = [];
      teardown();
      setLevel(void 0);
      if (mountedRef.current) void transcribe(blob);
    };
    recorderRef.current = recorder;
    streamRef.current = stream;
    recorder.start(250);
    const startedAt = Date.now();
    setElapsedMs(0);
    setStatus("recording");
    if (showLevel) meter(stream);
    timersRef.current.tick = setInterval(() => setElapsedMs(Date.now() - startedAt), 250);
    timersRef.current.stop = setTimeout(stop, maxDurationMs);
  }, [fail, maxDurationMs, meter, showLevel, stop, teardown, transcribe]);
  if (!supported) return null;
  return /* @__PURE__ */ jsx(
    VoiceInputButtonView,
    {
      className,
      disabled,
      elapsedMs,
      error,
      labels: labelOverrides,
      level: status === "recording" ? level : void 0,
      maxDurationMs,
      onPress: () => {
        if (status === "recording") stop();
        else void start();
      },
      status
    }
  );
}
export {
  DEFAULT_MAX_RECORDING_MS,
  RECORDER_MIME_CANDIDATES,
  VoiceInputButton,
  VoiceInputButtonView,
  describeMicError,
  formatElapsed,
  insertDictation,
  isVoiceInputSupported,
  pickRecorderMimeType
};
