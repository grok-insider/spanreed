import * as React from "react";
import { invoke } from "@tauri-apps/api/core";
import { ExternalLink } from "lucide-react";
import { Button, Card, PageHeader, SectionHeader, Tabs, TabsContent, TabsList, TabsTrigger } from "@fabrials/ui";
import { FabrialsLink } from "./fabrials-link";
import { MigrationPage } from "./migration";
import { NotificationSettings } from "./notifications";
import { SharingSettings } from "./sharing-controls";
import { ErrorAlert } from "./feedback";
import { routeHref } from "./routes";
import type { ThemePreference } from "./theme";
import type { LinkView } from "./contracts";

const sections = [
  { id: "general", label: "General" },
  { id: "account", label: "Fabrials account" },
  { id: "sharing", label: "Sharing & sync" },
  { id: "transfer", label: "Transfer accounts" },
] as const;
const themes: { id: ThemePreference; label: string }[] = [{ id: "system", label: "System" }, { id: "light", label: "Light" }, { id: "dark", label: "Dark" }];

export function SettingsPage({ tab, theme, onThemeChange }: { tab: string | null; theme: ThemePreference; onThemeChange: (theme: ThemePreference) => void }) {
  const current = sections.some((section) => section.id === tab) ? tab! : "general";
  const [linked, setLinked] = React.useState<boolean | null>(null);
  const [error, setError] = React.useState<string | null>(null);
  React.useEffect(() => {
    void invoke<LinkView>("fabrials_status").then((view) => setLinked(view.state === "linked")).catch(() => setLinked(null));
  }, []);
  return <>
    <PageHeader title="Settings" description="Appearance, reminders, your Fabrials account and what leaves this computer." />
    <Tabs value={current} onValueChange={(value) => { location.hash = routeHref({ workspace: "local", page: "settings", tab: String(value) }); }}>
      <TabsList aria-label="Settings sections">{sections.map((section) => <TabsTrigger key={section.id} value={section.id}>{section.label}</TabsTrigger>)}</TabsList>
      <TabsContent value="general" className="sr-stack">
        <Card className="sr-setting-card">
          <SectionHeader title="Appearance" description="Follow your system, or keep Spanreed light or dark. The window title bar follows too." />
          <fieldset className="sr-segmented">
            <legend className="fui-sr-only">Theme</legend>
            {themes.map((option) => <label key={option.id}>
              <input type="radio" name="theme" value={option.id} checked={theme === option.id} onChange={() => onThemeChange(option.id)} />
              <span>{option.label}</span>
            </label>)}
          </fieldset>
        </Card>
        <Card className="sr-setting-card">
          <SectionHeader title="Notifications" />
          <NotificationSettings />
        </Card>
      </TabsContent>
      <TabsContent value="account" className="sr-stack">
        <Card className="sr-setting-card">
          <SectionHeader title="Fabrials account" description="Optional. It lets you publish plan metrics, sync private history and use the hosted relay from this app." />
          <FabrialsLink onChange={(view) => setLinked(view.state === "linked")} />
        </Card>
        <Card className="sr-setting-card">
          <SectionHeader title="Hosted relay" description="ai.fabrials.com runs your accounts on a server so tools work from anywhere. Switch to Hosted relay at the top of the sidebar to manage it here." />
          <div className="fui-actions"><Button variant="outline" onClick={() => void invoke("open_hosted").catch((error) => setError(String(error)))}><ExternalLink aria-hidden size={16} />Open ai.fabrials.com</Button></div>
          <ErrorAlert title="Couldn't open the browser" error={error} />
        </Card>
      </TabsContent>
      <TabsContent value="sharing"><SharingSettings linked={linked} /></TabsContent>
      <TabsContent value="transfer"><MigrationPage /></TabsContent>
    </Tabs>
  </>;
}
