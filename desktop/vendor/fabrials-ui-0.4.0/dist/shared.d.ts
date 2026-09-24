export type StyledProps<T> = Omit<T, "className"> & {
    className?: string;
};
export declare function classes(...values: Array<string | false | null | undefined | ((...args: never[]) => string | undefined)>): string;
