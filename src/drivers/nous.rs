use crate::accounts;
use fabrials_providers::device_flow::Progress;

pub fn login_add(name: Option<&str>) -> Result<String, String> {
    let alias = name
        .map(str::to_string)
        .unwrap_or_else(|| accounts::unique_alias("nous", "portal"));
    let logins = crate::account_login::Logins::default();
    let login = logins.begin_nous(alias)?;
    eprintln!(
        "Open {}\nAuthorization code: {}",
        login.device.verification_uri, login.device.user_code
    );
    let mut interval = login.device.interval;
    loop {
        std::thread::sleep(std::time::Duration::from_secs(interval));
        match logins.poll_nous(&login.id)? {
            Progress::Pending { retry_after_secs } => interval = retry_after_secs,
            Progress::Connected => return Ok(format!("Connected nous/{}", login.alias)),
        }
    }
}

pub fn token(alias: &str) -> Result<String, String> {
    super::oauth::token("nous", alias)
}
