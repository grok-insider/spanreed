import * as React from "react";
import type { LucideIcon } from "lucide-react";
import { Cloud, Laptop, Menu, Settings2 } from "lucide-react";
import { Button, Sheet, SheetContent, SheetTitle, WorkspaceShell } from "@fabrials/ui";
import { FabrialBrandMark } from "./brand-mark";
import { DesktopWindowChrome } from "./window-chrome";
import { routeHref, type Route, type Workspace } from "./routes";

export type NavItem = { page: string; label: string; icon: LucideIcon; badge?: React.ReactNode };
export type NavGroup = { label: string; items: NavItem[] };

export const settingsItem: NavItem = { page: "settings", label: "Settings", icon: Settings2 };

const workspaces: { id: Workspace; label: string; icon: LucideIcon }[] = [
  { id: "local", label: "This computer", icon: Laptop },
  { id: "hosted", label: "Hosted relay", icon: Cloud },
];

function NavLink({ item, workspace, active, onNavigate }: { item: NavItem; workspace: Workspace; active: boolean; onNavigate?: () => void }) {
  const Icon = item.icon;
  return <a className="sr-nav-item" href={routeHref({ workspace, page: item.page })} aria-current={active ? "page" : undefined} onClick={onNavigate}>
    <Icon aria-hidden size={16} strokeWidth={1.75} />
    <span className="sr-nav-text">{item.label}</span>
    {item.badge}
  </a>;
}

function SidebarBody({ route, groups, footer, status, onNavigate }: { route: Route; groups: NavGroup[]; footer: NavItem[]; status?: React.ReactNode; onNavigate?: () => void }) {
  return <>
    <div className="sr-brand" data-tauri-drag-region>
      <FabrialBrandMark />
      <span className="sr-brand-name" data-tauri-drag-region>Spanreed</span>
    </div>
    <div className="sr-switcher" role="group" aria-label="Workspace">
      {workspaces.map(({ id, label, icon: Icon }) => <a key={id} className="sr-switcher-option" href={routeHref({ workspace: id, page: "overview" })} aria-current={route.workspace === id ? "true" : undefined} onClick={onNavigate}>
        <Icon aria-hidden size={14} strokeWidth={1.75} />{label}
      </a>)}
    </div>
    <nav className="sr-nav" aria-label="Primary">
      {groups.map((group) => <div className="sr-nav-group" key={group.label}>
        <p className="sr-nav-label">{group.label}</p>
        {group.items.map((item) => <NavLink key={item.page} item={item} workspace={route.workspace} active={route.page === item.page} onNavigate={onNavigate} />)}
      </div>)}
      <div className="sr-nav-group sr-nav-footer">
        {status && <div className="sr-sidebar-status">{status}</div>}
        {footer.map((item) => <NavLink key={item.page} item={item} workspace={route.workspace} active={route.page === item.page} onNavigate={onNavigate} />)}
      </div>
    </nav>
  </>;
}

export function DesktopShell({ route, groups, footer, status, actions, chrome = <DesktopWindowChrome />, children }: {
  route: Route;
  groups: NavGroup[];
  footer: NavItem[];
  status?: React.ReactNode;
  actions?: React.ReactNode;
  chrome?: React.ReactNode;
  children: React.ReactNode;
}) {
  const [drawer, setDrawer] = React.useState(false);
  const current = [...groups.flatMap((group) => group.items), ...footer].find((item) => item.page === route.page);
  return <WorkspaceShell
    className="sr-shell"
    navigation={<aside className="sr-sidebar"><SidebarBody route={route} groups={groups} footer={footer} status={status} /></aside>}
    header={<>
      <Button className="sr-menu-button" variant="ghost" size="icon" aria-label="Open navigation" onClick={() => setDrawer(true)}>
        <Menu aria-hidden size={18} />
      </Button>
      <div className="sr-titlebar" data-tauri-drag-region>
        <span className="sr-titlebar-title" data-tauri-drag-region>{current?.label}</span>
      </div>
      {actions && <div className="sr-header-actions">{actions}</div>}
      {chrome}
      <Sheet open={drawer} onOpenChange={setDrawer}>
        <SheetContent side="left" className="sr-drawer" closeLabel="Close navigation">
          <SheetTitle className="fui-sr-only">Navigation</SheetTitle>
          <SidebarBody route={route} groups={groups} footer={footer} status={status} onNavigate={() => setDrawer(false)} />
        </SheetContent>
      </Sheet>
    </>}
  >{children}</WorkspaceShell>;
}
