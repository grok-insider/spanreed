//! Identity / usage / route drivers. First-party in-process; same shape as a
//! future JSON-RPC session. The host owns accounts, secrets, probe, and the
//! proxy fabric — drivers only know how to login, refresh, and parse billing.

pub mod codex;
pub mod grok;
pub mod nous;
pub mod oauth;

/// `spanreed account …`
pub fn dispatch_account(args: &[String]) -> Result<String, String> {
    let sub = args.first().map(String::as_str).unwrap_or("ls");
    match sub {
        "ls" | "list" => Ok(cmd_ls()),
        "add" => {
            let provider = args
                .get(1)
                .ok_or("usage: spanreed account add <provider> [--name ALIAS]")?;
            let name = optional_name(args);
            match provider.as_str() {
                "grok" => grok::login_add(name.as_deref()),
                "nous" => nous::login_add(name.as_deref()),
                other => Err(format!("no identity driver for {other}")),
            }
        }
        "import" => {
            let provider = args
                .get(1)
                .ok_or("usage: spanreed account import <provider> [--name ALIAS]")?;
            let name = optional_name(args);
            match provider.as_str() {
                "grok" => grok::import_cli(name.as_deref()),
                other => Err(format!("no import for {other}")),
            }
        }
        "relabel" => grok::relabel_generic(),
        "alias" => cmd_alias(&args[1..]),
        "autosteer" => match args.get(1).map(String::as_str) {
            Some("on") => grok::autosteer_set(true),
            Some("off") => grok::autosteer_set(false),
            Some("status") | None => Ok(grok::autosteer_status()),
            Some(o) => Err(format!(
                "usage: spanreed account autosteer on|off|status (got {o})"
            )),
        },
        "use" => {
            let id = args
                .get(1)
                .ok_or("usage: spanreed account use <provider/alias>")?;
            let acc =
                crate::accounts::resolve(id).ok_or_else(|| format!("unknown account {id}"))?;
            let acc = crate::accounts::set_active(&acc.id)?;
            Ok(format!(
                "active {} is {}  (Grok Build via http://127.0.0.1:18736 — no auth.json rewrite)",
                acc.provider, acc.id
            ))
        }
        "login" => {
            let id = args
                .get(1)
                .ok_or("usage: spanreed account login <provider/alias>")?;
            grok::login_refresh(id)
        }
        "rm" | "remove" => {
            let id = args
                .get(1)
                .ok_or("usage: spanreed account rm <provider/alias>")?;
            let acc =
                crate::accounts::resolve(id).ok_or_else(|| format!("unknown account {id}"))?;
            crate::accounts::remove(&acc.id)?;
            Ok(format!("removed {}", acc.id))
        }
        "help" | "-h" | "--help" => Ok(account_help().into()),
        other => Err(format!(
            "unknown account subcommand: {other}\n{}",
            account_help()
        )),
    }
}

fn cmd_alias(args: &[String]) -> Result<String, String> {
    match args.first().map(String::as_str) {
        Some("rm") => {
            let nick = args
                .get(1)
                .ok_or("usage: spanreed account alias rm <nick>")?;
            let acc = crate::accounts::rm_nick(nick)?;
            Ok(format!("removed alias {nick} from {}", acc.id))
        }
        Some(id) => {
            let nick = args
                .get(1)
                .ok_or("usage: spanreed account alias <id> <nick>")?;
            let acc =
                crate::accounts::resolve(id).ok_or_else(|| format!("unknown account {id}"))?;
            let acc = crate::accounts::add_nick(&acc.id, nick)?;
            Ok(format!("{} also as {nick}  (/acct/{nick}/v1)", acc.id))
        }
        None => Err("usage: spanreed account alias <id> <nick> | alias rm <nick>".into()),
    }
}

fn optional_name(args: &[String]) -> Option<String> {
    if let Some(n) = args
        .iter()
        .position(|a| a == "--name")
        .and_then(|i| args.get(i + 1))
        .cloned()
    {
        return Some(n);
    }
    // `account add grok heavy` — positional alias, skip flags
    args.iter().skip(2).find(|a| !a.starts_with('-')).cloned()
}

fn cmd_ls() -> String {
    for a in crate::accounts::list_provider("grok") {
        grok::refresh_snapshot(&a);
    }
    let reg = crate::accounts::load();
    if reg.accounts.is_empty() {
        return "no accounts  (spanreed account add grok --name mine)\n".into();
    }
    format_ls(&reg.accounts)
}

pub fn format_ls(accounts: &[crate::accounts::Account]) -> String {
    let mut rows: Vec<[String; 7]> = vec![[
        " ".into(),
        "ID".into(),
        "PLAN".into(),
        "USED".into(),
        "BILLING".into(),
        "RENEWS".into(),
        "ALIASES".into(),
    ]];
    for a in accounts {
        let star = if a.active { "*" } else { " " };
        let plan = a
            .plan_label
            .as_deref()
            .or(a.plan_slug.as_deref())
            .unwrap_or("—");
        let used = a
            .used_pct
            .map(|p| format!("{p:.0}%"))
            .unwrap_or_else(|| "—".into());
        let billing = a.billing_interval.as_deref().unwrap_or("—");
        let renews = fmt_renews(
            a.renews_at.as_deref(),
            a.billing_interval.as_deref(),
            a.cancel_at_period_end,
        );
        let nicks = if a.aliases.is_empty() {
            "—".into()
        } else {
            a.aliases.join(",")
        };
        rows.push([
            star.into(),
            a.id.clone(),
            plan.into(),
            used,
            billing.into(),
            renews,
            nicks,
        ]);
    }
    let mut w = [1usize; 7];
    for r in &rows {
        for (i, c) in r.iter().enumerate() {
            w[i] = w[i].max(c.chars().count());
        }
    }
    let mut out = String::new();
    for r in &rows {
        out.push_str(&format!(
            "{:<w0$}  {:<w1$}  {:<w2$}  {:>w3$}  {:<w4$}  {:<w5$}  {}\n",
            r[0],
            r[1],
            r[2],
            r[3],
            r[4],
            r[5],
            r[6],
            w0 = w[0],
            w1 = w[1],
            w2 = w[2],
            w3 = w[3],
            w4 = w[4],
            w5 = w[5],
        ));
    }
    out
}

fn fmt_renews(iso: Option<&str>, interval: Option<&str>, cancel: Option<bool>) -> String {
    let Some(iso) = iso else {
        return "—".into();
    };
    let with_year = interval.is_some_and(|i| i.contains("year"))
        || crate::util::parse_iso_dt(iso)
            .map(|t| t.year() != time::OffsetDateTime::now_utc().year())
            .unwrap_or(false);
    let pat = if with_year {
        "[day padding:none] [month repr:short] [year]"
    } else {
        "[day padding:none] [month repr:short]"
    };
    let day = crate::util::parse_iso_dt(iso)
        .and_then(|t| {
            let fmt = time::format_description::parse_borrowed::<2>(pat).ok()?;
            t.format(&fmt).ok()
        })
        .unwrap_or_else(|| iso.chars().take(10).collect());
    if cancel == Some(true) {
        format!("{day} ends")
    } else {
        day
    }
}

pub fn account_help() -> &'static str {
    "spanreed account — identities the host owns\n\n\
     \tspanreed account add grok [--name NICK]    Device-code; id=heavy-1, nick optional\n\
     \tspanreed account import grok [--name NICK]\n\
     \tspanreed account alias <id> <nick>         Extra name for /acct/nick and use\n\
     \tspanreed account alias rm <nick>\n\
     \tspanreed account relabel                   Generic names → heavy-1 / premium-plus-1\n\
     \tspanreed account autosteer on|off|status   Fail over /v1 when a pool is 100%\n\
     \tspanreed account ls\n\
     \tspanreed account use grok/ALIAS            Default route on the fabric\n\
     \tspanreed account login grok/ALIAS          Re-auth when refresh dies\n\
     \tspanreed account rm grok/ALIAS\n\n\
     Grok Build stays on http://127.0.0.1:18736 ; parallel sessions use\n\
     GROK_CLI_CHAT_PROXY_BASE_URL=http://127.0.0.1:18736/acct/ALIAS/v1"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::Account;

    #[test]
    fn ls_table_aligns_premium_plus() {
        let mut a = Account::new("grok", "premium-plus-1").unwrap();
        a.plan_label = Some("X Premium+".into());
        a.used_pct = Some(0.0);
        a.billing_interval = Some("month".into());
        a.renews_at = Some("2026-09-16T00:00:00Z".into());
        let mut b = Account::new("grok", "heavy-1").unwrap();
        b.active = true;
        b.plan_label = Some("SuperGrok Heavy".into());
        b.used_pct = Some(22.0);
        b.billing_interval = Some("year".into());
        b.renews_at = Some("2027-01-01T00:00:00Z".into());
        b.aliases = vec!["work".into()];
        let out = format_ls(&[b, a]);
        assert!(out.contains("RENEWS"), "{out}");
        assert!(!out.contains("RESETS"), "{out}");
        assert!(out.contains("grok/premium-plus-1"), "{out}");
        assert!(out.contains("X Premium+"), "{out}");
        assert!(out.contains("work"), "{out}");
        assert!(
            out.contains("2027") || out.contains("1 Jan 2027"),
            "annual renews should include year\n{out}"
        );
        let lines: Vec<_> = out.lines().collect();
        assert!(lines.len() >= 3);
        // ID column: header and data start at the same offset
        let hid = lines[0].find("ID").unwrap();
        assert_eq!(lines[1].find("grok/heavy-1"), Some(hid));
        assert_eq!(lines[2].find("grok/premium-plus-1"), Some(hid));
    }
}
