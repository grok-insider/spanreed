import { type ComponentProps, type ReactNode } from "react";
export declare function PageHeader({ title, description, actions, eyebrow, className, }: {
    title: ReactNode;
    description?: ReactNode;
    actions?: ReactNode;
    /** Context above the title, e.g. a Breadcrumb. Sentence case, not a tracked label. */
    eyebrow?: ReactNode;
    className?: string;
}): import("react").JSX.Element;
export declare function SectionHeader({ title, description, actions, className, }: {
    title: ReactNode;
    description?: ReactNode;
    actions?: ReactNode;
    className?: string;
}): import("react").JSX.Element;
export declare function CollectionToolbar({ search, filters, actions, label, }: {
    search?: ReactNode;
    filters?: ReactNode;
    actions?: ReactNode;
    label?: string;
}): import("react").JSX.Element;
export declare function BulkActions({ count, children, label, regionLabel, }: {
    count: number;
    children: ReactNode;
    label?: ReactNode;
    regionLabel?: string;
}): import("react").JSX.Element | null;
declare const stateIcons: {
    loading: import("react").ForwardRefExoticComponent<Omit<import("lucide-react").LucideProps, "ref"> & import("react").RefAttributes<SVGSVGElement>>;
    empty: import("react").ForwardRefExoticComponent<Omit<import("lucide-react").LucideProps, "ref"> & import("react").RefAttributes<SVGSVGElement>>;
    error: import("react").ForwardRefExoticComponent<Omit<import("lucide-react").LucideProps, "ref"> & import("react").RefAttributes<SVGSVGElement>>;
    stale: import("react").ForwardRefExoticComponent<Omit<import("lucide-react").LucideProps, "ref"> & import("react").RefAttributes<SVGSVGElement>>;
    offline: import("react").ForwardRefExoticComponent<Omit<import("lucide-react").LucideProps, "ref"> & import("react").RefAttributes<SVGSVGElement>>;
    success: import("react").ForwardRefExoticComponent<Omit<import("lucide-react").LucideProps, "ref"> & import("react").RefAttributes<SVGSVGElement>>;
};
export declare function StatePanel({ state, title, description, actions, headingLevel, className, icon, align, }: {
    state: keyof typeof stateIcons;
    title: ReactNode;
    description?: ReactNode;
    actions?: ReactNode;
    headingLevel?: 2 | 3 | 4;
    className?: string;
    icon?: ReactNode;
    align?: "start" | "center";
}): import("react").JSX.Element;
export declare function Field({ label, description, error, children, id: providedId, }: {
    label: ReactNode;
    description?: ReactNode;
    error?: ReactNode;
    id?: string;
    children: (props: {
        id: string;
        "aria-describedby"?: string;
        "aria-invalid"?: true;
    }) => ReactNode;
}): import("react").JSX.Element;
export declare function WorkspaceShell({ navigation, header, children, contentId, skipLabel, className, ...props }: ComponentProps<"div"> & {
    navigation?: ReactNode;
    header?: ReactNode;
    contentId?: string;
    skipLabel?: string;
}): import("react").JSX.Element;
export declare function SiteHeader({ brand, navigation, actions, mobileMenu, sticky, className, ...props }: Omit<ComponentProps<"header">, "children"> & {
    brand: ReactNode;
    navigation?: ReactNode;
    actions?: ReactNode;
    /** Shown instead of the navigation below 768px, e.g. a Sheet trigger. */
    mobileMenu?: ReactNode;
    sticky?: boolean;
}): import("react").JSX.Element;
export declare function AuthLayout({ brand, title, description, children, footer, aside, headingLevel, className, }: {
    brand?: ReactNode;
    title: ReactNode;
    description?: ReactNode;
    children: ReactNode;
    /** A short note about what the session can and cannot do. */
    footer?: ReactNode;
    /** Optional editorial panel shown beside the card on wide screens. */
    aside?: ReactNode;
    headingLevel?: 1 | 2;
    className?: string;
}): import("react").JSX.Element;
export {};
