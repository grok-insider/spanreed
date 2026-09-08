import * as React from "react";
import { HostedMigration, type HostedMigrationApi } from "@fabrials/ui";
import { RemoteDeviceLogin } from "./remote-device-login";
import { useRemote } from "./remote-api";
export function RemoteMigration() {
  const remote = useRemote();
  const api = React.useMemo<HostedMigrationApi>(()=>({
    sessions:()=>remote("migrationSessions"), create:request=>remote("beginMigration",request),
    status:request=>remote("migrationStatus",request), approve:request=>remote("approveMigration",request),
    authorizations:request=>remote("migrationAuthorizations",request), forget:request=>remote("forgetMigration",request),
    cancel:request=>remote("cancelMigration",request),
  }),[remote]);
  return <HostedMigration api={api} origin="https://ai.fabrials.com" authorize={request=><RemoteDeviceLogin provider={request.provider} initialAlias={request.alias} migration={{migrationId:request.migrationId,sourceId:request.sourceId}} onConnected={request.onConnected}/>}/>;
}
