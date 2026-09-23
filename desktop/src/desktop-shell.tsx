import * as React from "react";
import type { LucideIcon } from "lucide-react";
import { Cloud, Laptop, Settings2 } from "lucide-react";
import {
  ProductLockup, Sidebar, SidebarContent, SidebarFooter, SidebarGroup, SidebarGroupLabel, SidebarHeader,
  SidebarInset, SidebarMenu, SidebarMenuBadge, SidebarMenuButton, SidebarMenuItem, SidebarProvider, SidebarRail, SidebarTrigger,
  ThemeSwitcher, useSidebar,
} from "@fabrials/ui";
import { DesktopWindowChrome } from "./window-chrome";
import { routeHref, type Route, type Workspace } from "./routes";
import type { ThemePreference } from "./theme";

export type NavItem = { page: string; label: string; icon: LucideIcon; badge?: React.ReactNode };
export type NavGroup = { label: string; items: NavItem[] };

export const settingsItem: NavItem = { page: "settings", label: "Settings", icon: Settings2 };

const workspaces: { id: Workspace; label: string; icon: LucideIcon }[] = [
  { id: "local", label: "This computer", icon: Laptop },
  { id: "hosted", label: "Hosted relay", icon: Cloud },
];

function NavLinks({ items, workspace, page }: { items: NavItem[]; workspace: Workspace; page: string }) {
  const { setOpenMobile } = useSidebar();
  return <SidebarMenu>
    {items.map((item) => {
      const Icon = item.icon;
      return <SidebarMenuItem key={item.page}>
        <SidebarMenuButton isActive={page === item.page} render={<a href={routeHref({ workspace, page: item.page })} onClick={() => setOpenMobile(false)} />}>
          <Icon aria-hidden size={16} strokeWidth={1.75} />
          <span>{item.label}</span>
        </SidebarMenuButton>
        {item.badge && <SidebarMenuBadge>{item.badge}</SidebarMenuBadge>}
      </SidebarMenuItem>;
    })}
  </SidebarMenu>;
}

export function DesktopShell({ route, groups, footer, status, actions, chrome = <DesktopWindowChrome />, theme, onThemeChange, children }: {
  route: Route;
  groups: NavGroup[];
  footer: NavItem[];
  status?: React.ReactNode;
  actions?: React.ReactNode;
  chrome?: React.ReactNode;
  theme?: ThemePreference;
  onThemeChange?: (theme: ThemePreference) => void;
  children: React.ReactNode;
}) {
  const current = [...groups.flatMap((group) => group.items), ...footer].find((item) => item.page === route.page);
  return <SidebarProvider className="sr-shell">
    <a className="fui-skip-link" href="#main-content">Skip to content</a>
    <Sidebar>
      <SidebarHeader data-tauri-drag-region>
        <ProductLockup product="Spanreed" gem="ruby" size="sm" />
        <div className="sr-switcher" role="group" aria-label="Workspace">
          {workspaces.map(({ id, label, icon: Icon }) => <a key={id} className="sr-switcher-option" href={routeHref({ workspace: id, page: "overview" })} aria-current={route.workspace === id ? "true" : undefined}>
            <Icon aria-hidden size={14} strokeWidth={1.75} />{label}
          </a>)}
        </div>
      </SidebarHeader>
      <SidebarContent>
        {groups.map((group) => <SidebarGroup key={group.label}>
          <SidebarGroupLabel>{group.label}</SidebarGroupLabel>
          <NavLinks items={group.items} workspace={route.workspace} page={route.page} />
        </SidebarGroup>)}
      </SidebarContent>
      <SidebarFooter>
        {status}
        <NavLinks items={footer} workspace={route.workspace} page={route.page} />
      </SidebarFooter>
      <SidebarRail />
    </Sidebar>
    <SidebarInset>
      <header className="sr-window-header">
        <SidebarTrigger />
        <div className="sr-titlebar" data-tauri-drag-region>
          <span className="sr-titlebar-title" data-tauri-drag-region>{current?.label}</span>
        </div>
        <div className="sr-header-actions">
          {actions}
          {theme && onThemeChange && <ThemeSwitcher value={theme} onValueChange={onThemeChange} />}
          {chrome}
        </div>
      </header>
      <div className="sr-main" id="main-content" tabIndex={-1}>{children}</div>
    </SidebarInset>
  </SidebarProvider>;
}
