use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::install::{validate_destination, version_probe};

const LIMIT: u64 = 256 * 1024 * 1024;
const RECORD: &str = "transaction.json";

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Phase {
    Prepared,
    Switching,
    Installed,
    Committed,
    Restoring,
    Restored,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Record {
    schema: u32,
    generation: String,
    destination: String,
    version: String,
    next_sha256: String,
    previous_sha256: Option<String>,
    phase: Phase,
    helper_sha256: Option<String>,
    parent: Option<(u32, u64)>,
    #[serde(default)]
    last_error: Option<String>,
}

struct Store {
    directory: PathBuf,
    destination: PathBuf,
    _lock: File,
}

#[cfg(unix)]
impl Drop for Store {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self._lock);
    }
}

impl Store {
    fn open(destination: &Path) -> Result<Self, String> {
        validate_destination(destination)?;
        let parent = destination.parent().ok_or("missing install directory")?;
        let directory = parent.join(".spanreed-updater");
        if directory.exists() {
            validate_destination(&directory.join(RECORD))?;
        } else {
            fabrials_agent_host::instance::ensure_private_directory(&directory)
                .map_err(|error| format!("create private updater state: {error}"))?;
        }
        let lock_path = directory.join("lock");
        validate_destination(&lock_path)?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        let lock = options
            .open(&lock_path)
            .map_err(|error| format!("open update lock: {error}"))?;
        #[cfg(windows)]
        fabrials_agent_host::win_acl::set_owner_only(&lock_path)
            .map_err(|error| error.to_string())?;
        FileExt::try_lock_exclusive(&lock).map_err(|_| "another updater or recovery is active")?;
        Ok(Self {
            directory,
            destination: destination.into(),
            _lock: lock,
        })
    }

    fn file(&self, record: &Record, role: &str) -> PathBuf {
        self.directory.join(format!(
            "{role}-{}{}",
            record.generation,
            if cfg!(windows) { ".exe" } else { "" }
        ))
    }

    fn read(&self) -> Result<Option<Record>, String> {
        let path = self.directory.join(RECORD);
        let file = match File::open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("read update recovery state: {error}")),
        };
        validate_destination(&path)?;
        if file.metadata().map_err(|error| error.to_string())?.len() > 8192 {
            return Err("update recovery state exceeds its size limit".into());
        }
        let mut bytes = Vec::new();
        file.take(8193)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        if bytes.len() > 8192 {
            return Err("update recovery state grew beyond its limit".into());
        }
        let record: Record = serde_json::from_slice(&bytes)
            .map_err(|_| "update recovery state is invalid; preserve the recovery directory")?;
        let hash =
            |value: &str| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit());
        if record.schema != 1
            || record.generation.len() != 32
            || !record
                .generation
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            || self.destination.file_name().and_then(|name| name.to_str())
                != Some(record.destination.as_str())
            || super::parse_semver(&record.version).is_none()
            || !hash(&record.next_sha256)
            || record
                .previous_sha256
                .as_deref()
                .is_some_and(|value| !hash(value))
            || record
                .helper_sha256
                .as_deref()
                .is_some_and(|value| !hash(value))
        {
            return Err("update recovery state does not match this installation".into());
        }
        Ok(Some(record))
    }

    fn write(&self, record: &Record) -> Result<(), String> {
        let pending = self.directory.join("transaction.next");
        if pending.exists() {
            validate_destination(&pending)?;
            fs::remove_file(&pending).map_err(|error| error.to_string())?;
        }
        let mut file = private_create(&pending)?;
        let bytes = serde_json::to_vec(record).map_err(|error| error.to_string())?;
        file.write_all(&bytes)
            .and_then(|()| file.sync_all())
            .map_err(|error| format!("persist update recovery state: {error}"))?;
        drop(file);
        replace(&pending, &self.directory.join(RECORD))
            .map_err(|error| format!("publish update recovery state: {error}"))?;
        flush_directory(&self.directory)?;
        #[cfg(test)]
        checkpoint(&format!("{:?}", record.phase));
        Ok(())
    }

    fn clean(&self, keep: Option<&Record>) -> Result<(), String> {
        let mut entries = fs::read_dir(&self.directory).map_err(|error| error.to_string())?;
        for _ in 0..64 {
            let Some(entry) = entries.next() else {
                return Ok(());
            };
            let entry = entry.map_err(|error| error.to_string())?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if name == "lock"
                || name == RECORD
                || keep.is_some_and(|record| name.contains(&record.generation))
            {
                continue;
            }
            if ["candidate-", "previous-", "helper-", "restore-"]
                .iter()
                .any(|prefix| name.starts_with(prefix))
                || name == "transaction.next"
            {
                validate_destination(&entry.path())?;
                match fs::remove_file(entry.path()) {
                    Ok(()) => {}
                    #[cfg(windows)]
                    Err(error) if matches!(error.raw_os_error(), Some(5 | 32 | 33)) => {}
                    Err(error) => return Err(format!("clean updater state: {error}")),
                }
            }
        }
        Err("too many retained updater files; finish recovery before installing again".into())
    }

    fn prepare(&self, source: &Path, version: &str) -> Result<Record, String> {
        let existing = self.read()?;
        if existing
            .as_ref()
            .is_some_and(|record| !matches!(record.phase, Phase::Committed | Phase::Restored))
        {
            return Err("an interrupted update needs recovery; run spanreed self-update --recover before another update".into());
        }
        if let Some(record) = &existing {
            let expected = if record.phase == Phase::Committed {
                Some(record.next_sha256.as_str())
            } else {
                record.previous_sha256.as_deref()
            };
            if digest_optional(&self.destination)?.as_deref() != expected {
                return Err("installed executable differs from recorded update state; automatic replacement refused".into());
            }
        }
        self.clean(existing.as_ref())?;
        let mut random = [0u8; 16];
        getrandom::getrandom(&mut random).map_err(|error| error.to_string())?;
        let generation = random.iter().map(|byte| format!("{byte:02x}")).collect();
        let record = Record {
            schema: 1,
            generation,
            destination: self
                .destination
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or("invalid install filename")?
                .into(),
            version: version.into(),
            next_sha256: digest(source)?,
            previous_sha256: digest_optional(&self.destination)?,
            phase: Phase::Prepared,
            helper_sha256: None,
            parent: None,
            last_error: None,
        };
        copy_verified(
            source,
            &self.file(&record, "candidate"),
            &record.next_sha256,
        )?;
        if let Some(hash) = &record.previous_sha256 {
            copy_verified(&self.destination, &self.file(&record, "previous"), hash)?;
        }
        flush_directory(&self.directory)?;
        self.write(&record)?;
        Ok(record)
    }

    fn install(&self, record: &mut Record) -> Result<(), String> {
        if record.phase != Phase::Prepared {
            return Err("update is not in its prepared state; use recovery".into());
        }
        if digest_optional(&self.destination)? != record.previous_sha256 {
            return Err("installed executable changed after staging; replacement refused".into());
        }
        let staged = self.file(record, "candidate");
        if digest(&staged)? != record.next_sha256 {
            return Err("staged executable failed its recorded checksum".into());
        }
        validate_destination(&self.destination)?;
        version_probe(&staged, &record.version, Duration::from_secs(5))?;
        record.phase = Phase::Switching;
        self.write(record)?;
        let result: Result<(), String> = (|| {
            replace(&staged, &self.destination)
                .map_err(|error| format!("replace installed executable: {error}"))?;
            flush_directory(self.destination.parent().ok_or("missing install parent")?)?;
            #[cfg(test)]
            checkpoint("Replaced");
            record.phase = Phase::Installed;
            self.write(record)?;
            if digest(&self.destination)? != record.next_sha256 {
                return Err("installed executable changed during replacement".into());
            }
            version_probe(&self.destination, &record.version, Duration::from_secs(5))?;
            record.phase = Phase::Committed;
            self.write(record)?;
            Ok(())
        })();
        match result {
            Ok(()) => {
                if let Err(error) = self.clean(Some(record)) {
                    eprintln!("update committed; cleanup deferred: {error}");
                }
                Ok(())
            }
            Err(error) => match self.restore(record) {
                Ok(()) => Err(format!("{error}; previous installation restored")),
                Err(restore) => Err(format!(
                    "{error}; rollback failed: {restore}; recovery files retained at {}",
                    self.directory.display()
                )),
            },
        }
    }

    fn restore(&self, record: &mut Record) -> Result<(), String> {
        let current = digest_optional(&self.destination)?;
        if current != record.previous_sha256
            && current.as_deref() != Some(record.next_sha256.as_str())
            && current.is_some()
        {
            return Err(
                "installed executable has an unknown checksum; recovery will not overwrite it"
                    .into(),
            );
        }
        record.phase = Phase::Restoring;
        self.write(record)?;
        if current != record.previous_sha256 {
            if let Some(hash) = &record.previous_sha256 {
                let previous = self.file(record, "previous");
                let staged = self.file(record, "restore");
                if staged.exists() {
                    validate_destination(&staged)?;
                    fs::remove_file(&staged).map_err(|error| error.to_string())?;
                }
                copy_verified(&previous, &staged, hash)?;
                replace(&staged, &self.destination)
                    .map_err(|error| format!("restore previous executable: {error}"))?;
            } else if self.destination.exists() {
                fs::remove_file(&self.destination)
                    .map_err(|error| format!("remove rejected first installation: {error}"))?;
            }
            flush_directory(self.destination.parent().ok_or("missing install parent")?)?;
        }
        if digest_optional(&self.destination)? != record.previous_sha256 {
            return Err("restored executable failed verification".into());
        }
        record.phase = Phase::Restored;
        self.write(record)?;
        Ok(())
    }
}

fn private_create(path: &Path) -> Result<File, String> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o700).custom_flags(libc::O_NOFOLLOW);
    }
    let file = options
        .open(path)
        .map_err(|error| format!("create updater file: {error}"))?;
    #[cfg(windows)]
    fabrials_agent_host::win_acl::set_owner_only(path).map_err(|error| error.to_string())?;
    Ok(file)
}

fn copy_verified(source: &Path, destination: &Path, expected: &str) -> Result<(), String> {
    validate_destination(source)?;
    let input = File::open(source).map_err(|error| error.to_string())?;
    if input.metadata().map_err(|error| error.to_string())?.len() > LIMIT {
        return Err("update executable exceeds 256 MiB".into());
    }
    let mut output = private_create(destination)?;
    std::io::copy(&mut input.take(LIMIT + 1), &mut output).map_err(|error| error.to_string())?;
    output
        .sync_all()
        .map_err(|error| format!("persist update executable: {error}"))?;
    drop(output);
    if digest(destination)? != expected {
        return Err("copied executable failed its recorded checksum".into());
    }
    Ok(())
}

fn digest_optional(path: &Path) -> Result<Option<String>, String> {
    match fs::symlink_metadata(path) {
        Ok(_) => digest(path).map(Some),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("inspect update executable: {error}")),
    }
}

fn digest(path: &Path) -> Result<String, String> {
    validate_destination(path)?;
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    if file.metadata().map_err(|error| error.to_string())?.len() > LIMIT {
        return Err("update executable exceeds 256 MiB".into());
    }
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 16384];
    let mut read = 0u64;
    loop {
        let count = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        read += count as u64;
        if read > LIMIT {
            return Err("update executable grew beyond its limit".into());
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn replace(source: &Path, destination: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        super::windows::replace(source, destination)
    }
    #[cfg(not(windows))]
    {
        fs::rename(source, destination)
    }
}

fn flush_directory(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(|error| format!("flush update directory: {error}"))?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

pub(super) fn install(destination: &Path, staged: &Path, version: &str) -> Result<(), String> {
    let store = Store::open(destination)?;
    let mut record = store.prepare(staged, version)?;
    #[cfg(not(windows))]
    {
        store.install(&mut record)
    }
    #[cfg(windows)]
    {
        schedule(&store, &mut record)
    }
}

pub(super) fn status(destination: &Path) -> Result<String, String> {
    let directory = destination
        .parent()
        .ok_or("missing install directory")?
        .join(".spanreed-updater");
    if !directory.exists() {
        return Ok("no recorded update transaction".into());
    }
    let store = Store::open(destination)?;
    let Some(record) = store.read()? else {
        return Ok("no recorded update transaction".into());
    };
    let current = digest_optional(destination)?;
    let installation = if current.as_deref() == Some(record.next_sha256.as_str()) {
        "selected update"
    } else if current == record.previous_sha256 {
        "previous installation"
    } else {
        "unknown checksum; replacement refused"
    };
    let error = record
        .last_error
        .map(|value| format!("; last failure: {value}"))
        .unwrap_or_default();
    Ok(format!(
        "update {}: {:?}; executable: {installation}{error}",
        record.version, record.phase
    ))
}

pub(super) fn recover(destination: &Path, rollback: bool) -> Result<String, String> {
    let store = Store::open(destination)?;
    let Some(mut record) = store.read()? else {
        return Ok("no interrupted update or retained rollback".into());
    };
    if record.phase == Phase::Committed && !rollback {
        if digest_optional(destination)?.as_deref() != Some(record.next_sha256.as_str()) {
            return Err("installed executable differs from the committed update".into());
        }
        return Ok("update committed; --rollback restores its retained previous executable".into());
    }
    if rollback && record.previous_sha256.is_none() {
        return Err("this update has no previous executable to restore".into());
    }
    #[cfg(windows)]
    if std::env::current_exe().map_err(|error| error.to_string())? != store.file(&record, "helper")
    {
        record.phase = Phase::Restoring;
        store.write(&record)?;
        schedule(&store, &mut record)?;
        return Ok("recovery prepared; the verified helper restores the previous executable after this process exits".into());
    }
    store.restore(&mut record)?;
    Ok(
        "previous installation restored and verified; identity and pairing data were preserved"
            .into(),
    )
}

#[cfg(windows)]
pub(super) fn finish() -> Result<String, String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let directory = executable.parent().ok_or("missing helper directory")?;
    if directory.file_name().and_then(|name| name.to_str()) != Some(".spanreed-updater") {
        return Err("update completion is only available from the verified private helper".into());
    }
    let destination = directory
        .parent()
        .ok_or("missing install directory")?
        .join("spanreed.exe");
    let pending = {
        validate_destination(&directory.join(RECORD))?;
        let store = Store {
            directory: directory.into(),
            destination: destination.clone(),
            _lock: File::open(directory.join("lock")).map_err(|error| error.to_string())?,
        };
        let record = store.read()?.ok_or("missing update recovery state")?;
        if store.file(&record, "helper") != executable
            || record.helper_sha256.as_deref() != Some(digest(&executable)?.as_str())
        {
            return Err("update helper does not match the prepared transaction".into());
        }
        record
    };
    let (pid, created) = pending.parent.ok_or("missing update parent identity")?;
    super::windows::wait_parent(pid, created)?;
    let host_directory = fabrials_agent_host::state_dir::resolve_state_dirs()
        .map_err(|error| error.to_string())?
        .state_dir;
    let _host = super::install::claim_host(&host_directory)?;
    let store = Store::open(&destination)?;
    let mut record = store.read()?.ok_or("missing prepared update")?;
    if record.generation != pending.generation {
        return Err("prepared update was replaced before its helper started".into());
    }
    let result = if record.phase == Phase::Restoring {
        store
            .restore(&mut record)
            .map(|()| "previous installation restored and verified".into())
    } else {
        store
            .install(&mut record)
            .map(|()| "update committed and verified".into())
    };
    if let Err(error) = &result {
        record.last_error = Some(error.chars().take(1024).collect());
        store.write(&record)?;
    }
    result
}

#[cfg(windows)]
fn schedule(store: &Store, record: &mut Record) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let helper = store.file(record, "helper");
    let hash = digest(&executable)?;
    if helper.exists() {
        if digest(&helper)? != hash {
            validate_destination(&helper)?;
            fs::remove_file(&helper).map_err(|error| format!("replace update helper: {error}"))?;
            copy_verified(&executable, &helper, &hash)?;
        }
    } else {
        copy_verified(&executable, &helper, &hash)?;
    }
    record.last_error = None;
    record.helper_sha256 = Some(hash);
    record.parent = Some((
        std::process::id(),
        super::windows::process_stamp(std::process::id())?,
    ));
    store.write(record)?;
    std::process::Command::new(helper)
        .args(["self-update", "--finish"])
        .creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|error| format!("start update helper; run --recover before retrying: {error}"))?;
    Ok(())
}

#[cfg(test)]
fn checkpoint(phase: &str) {
    if std::env::var("SPANREED_UPDATE_TEST_CRASH").ok().as_deref() == Some(phase) {
        std::process::exit(73);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "spanreed-durable-update-{}",
                fabrials_fabric::accounting::new_request_id()
            ));
            fabrials_agent_host::instance::ensure_private_directory(&path).unwrap();
            Self(path)
        }
        fn destination(&self) -> PathBuf {
            self.0.join("spanreed")
        }
        fn source(&self) -> PathBuf {
            self.0.join("source")
        }
        fn seed(&self) {
            fs::write(self.destination(), b"previous executable").unwrap();
            let mut file = private_create(&self.source()).unwrap();
            file.write_all(b"#!/bin/sh\nprintf 'spanreed 1.2.3\\n'\n")
                .unwrap();
            file.sync_all().unwrap();
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn installation_lock_excludes_another_state_or_recovery() {
        let fixture = Fixture::new();
        let first = Store::open(&fixture.destination()).unwrap();
        assert!(Store::open(&fixture.destination()).is_err());
        drop(first);
        assert!(Store::open(&fixture.destination()).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn completed_owner_releases_lock_even_while_a_duplicate_handle_survives() {
        let fixture = Fixture::new();
        let first = Store::open(&fixture.destination()).unwrap();
        let inherited = first._lock.try_clone().unwrap();
        assert!(Store::open(&fixture.destination()).is_err());
        drop(first);
        let next = Store::open(&fixture.destination()).unwrap();
        drop(inherited);
        assert!(Store::open(&fixture.destination()).is_err());
        drop(next);
        assert!(Store::open(&fixture.destination()).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn crash_worker() {
        let Some(path) = std::env::var_os("SPANREED_UPDATE_TEST_DIRECTORY") else {
            return;
        };
        let fixture = Fixture(path.into());
        let store = Store::open(&fixture.destination()).unwrap();
        let mut record = store.prepare(&fixture.source(), "1.2.3").unwrap();
        store.install(&mut record).unwrap();
        store.restore(&mut record).unwrap();
        std::mem::forget(fixture);
        panic!("requested update checkpoint was never reached");
    }

    #[cfg(unix)]
    #[test]
    fn killed_process_recovers_every_persisted_transition_and_replace_gap() {
        for phase in [
            "Prepared",
            "Switching",
            "Replaced",
            "Installed",
            "Committed",
            "Restoring",
            "Restored",
        ] {
            let fixture = Fixture::new();
            fixture.seed();
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "self_update::durable::tests::crash_worker",
                    "--nocapture",
                ])
                .env("SPANREED_UPDATE_TEST_DIRECTORY", &fixture.0)
                .env("SPANREED_UPDATE_TEST_CRASH", phase)
                .output()
                .unwrap();
            assert_eq!(
                output.status.code(),
                Some(73),
                "{phase}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let result = recover(&fixture.destination(), false).unwrap();
            if phase == "Committed" {
                assert!(result.contains("committed"));
                assert_eq!(
                    fs::read(fixture.destination()).unwrap(),
                    fs::read(fixture.source()).unwrap()
                );
                recover(&fixture.destination(), true).unwrap();
            }
            assert_eq!(
                fs::read(fixture.destination()).unwrap(),
                b"previous executable",
                "{phase}"
            );
            recover(&fixture.destination(), false).unwrap();
            assert_eq!(
                fs::read(fixture.destination()).unwrap(),
                b"previous executable",
                "{phase}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn completed_update_keeps_a_verified_explicit_rollback() {
        let fixture = Fixture::new();
        fixture.seed();
        install(&fixture.destination(), &fixture.source(), "1.2.3").unwrap();
        assert_eq!(
            fs::read(fixture.destination()).unwrap(),
            fs::read(fixture.source()).unwrap()
        );
        assert!(
            recover(&fixture.destination(), false)
                .unwrap()
                .contains("committed")
        );
        recover(&fixture.destination(), true).unwrap();
        assert_eq!(
            fs::read(fixture.destination()).unwrap(),
            b"previous executable"
        );
        recover(&fixture.destination(), true).unwrap();
        assert_eq!(
            fs::read(fixture.destination()).unwrap(),
            b"previous executable"
        );
    }

    #[test]
    fn corrupt_recovery_record_never_adopts_an_arbitrary_path() {
        let fixture = Fixture::new();
        fixture.seed();
        let store = Store::open(&fixture.destination()).unwrap();
        let record = store.prepare(&fixture.source(), "1.2.3").unwrap();
        let mut invalid = record.clone();
        invalid.generation = "../outside".into();
        store.write(&invalid).unwrap();
        assert!(store.read().is_err());
        assert_eq!(
            fs::read(fixture.destination()).unwrap(),
            b"previous executable"
        );
    }

    #[test]
    fn backup_checksum_failure_preserves_current_executable_and_recovery_files() {
        let fixture = Fixture::new();
        fixture.seed();
        let store = Store::open(&fixture.destination()).unwrap();
        let mut record = store.prepare(&fixture.source(), "1.2.3").unwrap();
        replace(&store.file(&record, "candidate"), &fixture.destination()).unwrap();
        fs::write(store.file(&record, "previous"), b"corrupted backup").unwrap();
        assert!(store.restore(&mut record).is_err());
        assert_eq!(
            fs::read(fixture.destination()).unwrap(),
            fs::read(fixture.source()).unwrap()
        );
        assert!(store.file(&record, "previous").exists());
    }

    #[test]
    fn changed_destination_is_not_overwritten_by_recovery() {
        let fixture = Fixture::new();
        fixture.seed();
        let store = Store::open(&fixture.destination()).unwrap();
        let mut record = store.prepare(&fixture.source(), "1.2.3").unwrap();
        fs::write(fixture.destination(), b"unrelated newer installation").unwrap();
        assert!(store.restore(&mut record).is_err());
        assert_eq!(
            fs::read(fixture.destination()).unwrap(),
            b"unrelated newer installation"
        );
    }

    #[test]
    fn unfinished_update_prevents_another_candidate() {
        let fixture = Fixture::new();
        fixture.seed();
        let store = Store::open(&fixture.destination()).unwrap();
        store.prepare(&fixture.source(), "1.2.3").unwrap();
        assert!(
            store
                .prepare(&fixture.source(), "1.2.4")
                .unwrap_err()
                .contains("recovery")
        );
    }
}
