import type { ReactNode } from "react";
import { WorkspaceShell } from "@fabrials/ui";

type Item = { id: string; label: string };

/** Host adapter: Spanreed owns the active page, the fabrial mark and the Tauri window controls. */
export function DesktopShell({ title, navigation, active, onNavigate, actions, brandMark, chrome, children }: {
  title: string;
  navigation: Item[];
  active: string;
  onNavigate: (id: string) => void;
  actions?: ReactNode;
  brandMark?: ReactNode;
  chrome?: ReactNode;
  children: ReactNode;
}) {
  const current = navigation.find((item) => item.id === active)?.label;
  return <WorkspaceShell
    navigation={<aside className="fb-sidebar" data-tauri-drag-region>
      <div className="fb-brand" data-tauri-drag-region>{brandMark}<span>{title}<small>Fabrials</small></span></div>
      <nav aria-label="Workspace">{navigation.map((item) => <button key={item.id} type="button" aria-current={active === item.id ? "page" : undefined} onClick={() => onNavigate(item.id)}>{item.label}</button>)}</nav>
    </aside>}
    header={<><div className="fb-chrome-drag" data-tauri-drag-region><span className="fb-muted">{current}</span></div><div className="fb-chrome-actions">{actions}{chrome}</div></>}
  >{children}</WorkspaceShell>;
}
