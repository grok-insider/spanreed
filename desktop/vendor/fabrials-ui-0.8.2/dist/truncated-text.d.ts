export type TruncatedTextProps = {
    children: string;
    className?: string;
    side?: "top" | "right" | "bottom" | "left";
    sideOffset?: number;
    align?: "start" | "center" | "end";
};
export declare function TruncatedText({ children, className, side, sideOffset, align, }: TruncatedTextProps): import("react").JSX.Element;
