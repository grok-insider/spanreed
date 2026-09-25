import * as React from "react";
import type { MigrationDirection, MigrationInvitation, MigrationSessionView } from "./contracts";
export interface HostedMigrationApi {
    sessions(): Promise<{
        sessions: MigrationSessionView[];
    }>;
    create(request: {
        direction: MigrationDirection;
    }): Promise<MigrationInvitation>;
    status(request: {
        id: string;
    }): Promise<MigrationSessionView>;
    approve(request: {
        id: string;
        revision: string;
    }): Promise<unknown>;
    authorizations(request: {
        id: string;
    }): Promise<string[]>;
    forget(request: {
        id: string;
    }): Promise<unknown>;
    cancel(request: {
        id: string;
    }): Promise<unknown>;
}
export type MigrationAuthorization = {
    provider: "grok" | "nous" | "codex";
    alias: string;
    migrationId: string;
    sourceId: string;
    onConnected: () => Promise<void>;
};
export declare function HostedMigration({ api: migrationApi, origin, authorize, heading, }: {
    api: HostedMigrationApi;
    origin: string;
    authorize: (request: MigrationAuthorization) => React.ReactNode;
    /** The host already shows the page title. */
    heading?: boolean;
}): React.JSX.Element;
