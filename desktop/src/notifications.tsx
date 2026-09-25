import * as React from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button, Switch } from "@fabrials/ui";
import { Done, ErrorAlert } from "./feedback";
import type { ResetNotificationSettings } from "./contracts";

export function NotificationSettings() {
  const [enabled, setEnabled] = React.useState<boolean | null>(null);
  const [pending, setPending] = React.useState(false);
  const [notice, setNotice] = React.useState<string | null>(null);
  const [error, setError] = React.useState<string | null>(null);
  React.useEffect(() => {
    let active = true;
    void invoke<ResetNotificationSettings>("notification_settings").then((value) => { if (active) setEnabled(value.resetExpiry); })
      .catch((error) => { if (active) setError(String(error)); });
    return () => { active = false; };
  }, []);
  async function change(next: boolean) {
    setPending(true); setError(null); setNotice(null);
    try {
      await invoke("set_reset_notifications", { enabled: next });
      setEnabled(next);
      if (next) await invoke("check_reset_notifications");
      setNotice(next ? "Reset reminders are on." : "Reset reminders are off.");
    } catch (error) { setError(String(error)); }
    finally { setPending(false); }
  }
  async function test() {
    setPending(true); setError(null); setNotice(null);
    try { await invoke("test_reset_notification"); setNotice("Test sent. If you don't see it, check your system's notification settings or focus mode."); }
    catch (error) { setError(String(error)); }
    finally { setPending(false); }
  }
  return <div className="sr-setting">
    <div className="sr-setting-text">
      <label htmlFor="reset-reminders" className="sr-setting-title">Remind me before reset credits expire</label>
      <p className="fui-description">A system notification when a reported reset credit expires within 24 hours, while Spanreed is running. Your system may ask for permission. Nothing is shared.</p>
      <ErrorAlert title="Couldn't change reminders" error={error} />
      <Done>{notice}</Done>
      {enabled && <div className="fui-actions"><Button variant="outline" size="sm" disabled={pending} onClick={() => void test()}>Send a test notification</Button></div>}
    </div>
    <Switch id="reset-reminders" checked={enabled ?? false} disabled={pending || enabled === null} onCheckedChange={(checked) => void change(checked)} />
  </div>;
}
