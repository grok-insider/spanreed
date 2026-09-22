export type StyledProps<T> = Omit<T, "className"> & {
    className?: string;
};
export declare function classes(...values: (string | false | null | undefined)[]): string;
