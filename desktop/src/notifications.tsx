import * as React from "react";
import { invoke } from "@tauri-apps/api/core";
import type { ResetNotificationSettings } from "./contracts";

export function NotificationSettings() {
  const [enabled, setEnabled] = React.useState(false);
  const [saved, setSaved] = React.useState<boolean | null>(null);
  const [pending, setPending] = React.useState(false);
  const [notice, setNotice] = React.useState<string | null>(null);
  const [error, setError] = React.useState<string | null>(null);
  React.useEffect(() => {
    let active = true;
    void invoke<ResetNotificationSettings>("notification_settings").then(value => {
      if (active) { setEnabled(value.resetExpiry);setSaved(value.resetExpiry); }
    }).catch(error => { if (active) setError(String(error)); });
    return () => { active = false; };
  }, []);
  async function save(event: React.FormEvent) {
    event.preventDefault(); setPending(true);setError(null);setNotice(null);
    try {
      await invoke("set_reset_notifications", {enabled});setSaved(enabled);
      if (enabled) await invoke("check_reset_notifications");
    } catch (error) {setError(String(error));}
    finally {setPending(false);}
  }
  async function test() {
    setPending(true);setError(null);setNotice(null);
    try {
      await invoke("test_reset_notification");
      setNotice("Test sent to the operating system. Check your notifications; focus mode may hide the banner.");
    } catch (error) {setError(String(error));}
    finally {setPending(false);}
  }
  return <section className="fb-card"><header><h2>Reset notifications</h2><p className="fb-muted">Get a system notification when reported credits expire within 24 hours, while Spanreed is running.</p></header>
    <form className="fb-card-body fb-form" onSubmit={event => void save(event)}>
      <label><span><input type="checkbox" checked={enabled} disabled={pending || saved === null} onChange={event => setEnabled(event.target.checked)} /> Notify me before reset credits expire</span></label>
      <p className="fb-muted">Enabling may request permission from your operating system. This preference does not publish or synchronize data.</p>
      <button className="fb-button" type="submit" disabled={pending || saved === null || saved === enabled}>{pending ? "Saving…" : "Save notification preference"}</button>
      <button className="fb-button" type="button" disabled={pending || saved !== true || !enabled} onClick={()=>void test()}>Send test notification</button>
      {notice && <p role="status">{notice}</p>}
      {saved !== null && <p role="status">{saved ? "Reset notifications enabled." : "Reset notifications disabled."}</p>}
      {error && <p role="alert" className="fb-error">{error}</p>}
    </form>
  </section>;
}
