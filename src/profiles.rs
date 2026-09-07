//! Built-in status-bar integrations, rendered from the same cached snapshot.
use std::path::Path;
use std::process::ExitCode;

const WAYBAR: &str = r#"{
  "custom/spanreed": {
    "exec": "spanreed waybar", "return-type": "json", "interval": 120,
    "on-click": "spanreed gui", "tooltip": true
  }
}
"#;
const WAYBAR_CSS: &str = "#custom-spanreed { font-family: 'IBM Plex Sans'; padding: 0 12px; }\n#custom-spanreed.warning { color: #b7791f; }\n#custom-spanreed.critical, #custom-spanreed.error { color: #c53030; }\n";
const EWW: &str = r#"(defpoll spanreed :interval "120s" :initial "[]" "spanreed widget json")
(defwidget spanreed-usage []
  (box :orientation "v" :class "spanreed"
    (for provider in spanreed
      (box :orientation "v" :space-evenly false
        (label :text {provider.displayName} :halign "start")
        (for metric in {provider.lines}
          (label :text {metric.label + ": " + (metric.value ?: metric.text ?: "")} :halign "start"))))))
"#;
const EWW_CSS: &str = ".spanreed { font-family: 'IBM Plex Sans'; padding: 16px; }\n.spanreed label { padding: 4px 0; }\n";
const SKETCHYBAR: &str = r#"#!/bin/sh
# Source this file from sketchybarrc. Requires jq for JSON field selection.
sketchybar --add item spanreed right \
  --set spanreed update_freq=120 label.font='IBM Plex Sans:Regular:12.0' \
  script='spanreed waybar | jq -r .text | xargs -I{} sketchybar --set spanreed label="{}"' \
  click_script='spanreed gui'
"#;

fn files(name: &str) -> Option<Vec<(&'static str, &'static str)>> {
    match name {
        "waybar" => Some(vec![
            ("spanreed.jsonc", WAYBAR),
            ("spanreed.css", WAYBAR_CSS),
        ]),
        "eww" => Some(vec![("spanreed.yuck", EWW), ("spanreed.scss", EWW_CSS)]),
        "sketchybar" => Some(vec![("spanreed.sh", SKETCHYBAR)]),
        _ => None,
    }
}

fn install(name: &str, directory: &Path) -> Result<(), String> {
    use std::io::Write;
    let files = files(name).ok_or("Unknown profile")?;
    if files.iter().any(|(name, _)| directory.join(name).exists()) {
        return Err("A profile file already exists; choose an empty output directory".into());
    }
    std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    let mut created = Vec::new();
    for (name, contents) in files {
        let path = directory.join(name);
        let result = (|| {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?;
            created.push(path);
            file.write_all(contents.as_bytes())?;
            file.sync_all()
        })();
        if let Err(error) = result {
            for path in created {
                let _ = std::fs::remove_file(path);
            }
            return Err(error.to_string());
        }
    }
    Ok(())
}

pub fn cmd(args: &[String]) -> ExitCode {
    let result = match args.first().map(String::as_str) {
        None | Some("list") => {
            println!("waybar\neww\nsketchybar");
            return ExitCode::SUCCESS;
        }
        Some("show") if args.len() == 2 => files(&args[1])
            .ok_or_else(|| "Unknown profile".into())
            .map(|files| {
                for (name, body) in files {
                    println!("--- {name} ---\n{body}");
                }
            }),
        Some("install") if args.len() == 4 && args[2] == "--output" => {
            install(&args[1], Path::new(&args[3]))
        }
        _ => Err("Usage: spanreed profile list | show NAME | install NAME --output DIR".into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

pub fn widget(args: &[String]) -> ExitCode {
    if args.first().is_some_and(|arg| arg != "json") {
        eprintln!("Usage: spanreed widget json");
        return ExitCode::FAILURE;
    }
    let mut outputs = crate::desktop::snapshot(false);
    for output in &mut outputs {
        for line in &mut output.lines {
            if let crate::model::MetricLine::Progress {
                label,
                used,
                limit,
                format,
                ..
            } = line
            {
                let value = match format {
                    crate::model::ProgressFormat::Percent => format!("{used:.0}%"),
                    crate::model::ProgressFormat::Dollars => format!("${used:.2} / ${limit:.2}"),
                    crate::model::ProgressFormat::Count { suffix } => {
                        format!("{used:.0} / {limit:.0} {suffix}")
                    }
                };
                *line = crate::model::MetricLine::text(
                    crate::model::MetricKind::Quota,
                    label.clone(),
                    value,
                );
            }
        }
    }
    match serde_json::to_string(&outputs) {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(_) => ExitCode::FAILURE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn installation_does_not_replace_user_files() {
        let dir = std::env::temp_dir().join(format!("spanreed-profiles-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("spanreed.jsonc"), "user configuration").unwrap();
        assert!(install("waybar", &dir).is_err());
        assert_eq!(
            std::fs::read_to_string(dir.join("spanreed.jsonc")).unwrap(),
            "user configuration"
        );
        assert!(!dir.join("spanreed.css").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
