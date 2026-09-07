//! Private native pairing state. Persist proof before consuming an invitation.
use fabrials_core::migration::MigrationSessionView;
use fabrials_runtime::{
    file_set::{Change, FileSet},
    migration::client::Client,
};
use serde::{Deserialize, Serialize};
use std::{
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Serialize, Deserialize)]
struct Record {
    id: String,
    origin: String,
    proof: String,
    local_environment: String,
    view: Option<MigrationSessionView>,
    #[serde(default)]
    transfer: Option<Vec<fabrials_accounts::transfer::ApiKeyTransfer>>,
    #[serde(default)]
    imported: Option<Vec<String>>,
    #[serde(default)]
    oauth_generations: std::collections::BTreeMap<String, String>,
}
#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedSession {
    pub id: String,
    pub origin: String,
    pub view: Option<MigrationSessionView>,
    pub imported: Option<Vec<String>>,
}
/// Local metadata only. Listing never contacts the hosted environment.
pub fn saved() -> Result<Vec<SavedSession>, String> {
    saved_at(&root())
}
fn saved_at(directory: &Path) -> Result<Vec<SavedSession>, String> {
    let _files = FileSet::acquire_wait(directory)?;
    let mut sessions = Vec::new();
    for (count, entry) in std::fs::read_dir(directory)
        .map_err(|_| "Migration directory unavailable")?
        .enumerate()
    {
        if count >= 4096 {
            return Err("Migration directory contains too many entries".into());
        }
        let entry = entry.map_err(|_| "Migration state unavailable")?;
        let name = entry.file_name();
        let Some(id) = name.to_str().and_then(|name| name.strip_suffix(".json")) else {
            continue;
        };
        if filename(id).is_err() {
            continue;
        }
        if sessions.len() >= 256 {
            return Err("Too many saved migration sessions".into());
        }
        if let Some(record) = load(directory, id)? {
            sessions.push(SavedSession {
                id: record.id,
                origin: record.origin,
                view: record.view,
                imported: record.imported,
            });
        }
    }
    sessions.sort_by(|a, b| {
        b.view
            .as_ref()
            .map(|view| view.expires_at_ms)
            .cmp(&a.view.as_ref().map(|view| view.expires_at_ms))
            .then(a.id.cmp(&b.id))
    });
    Ok(sessions)
}

fn root() -> PathBuf {
    crate::app::data_dir().join("migration-sessions")
}
fn filename(id: &str) -> Result<String, String> {
    if !(32..=128).contains(&id.len())
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
    {
        return Err("Invalid migration session ID".into());
    }
    Ok(format!("{id}.json"))
}
fn load(root: &Path, id: &str) -> Result<Option<Record>, String> {
    let path = root.join(filename(id)?);
    match std::fs::symlink_metadata(&path) {
        Ok(meta) if !meta.is_file() => return Err("Migration state must be a regular file".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("Migration state unavailable".into()),
        _ => {}
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|_| "Migration state unavailable")?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Migration state unavailable")?;
    if bytes.len() > 1024 * 1024 {
        return Err("Migration state exceeds size limit".into());
    }
    let record: Record = serde_json::from_slice(&bytes).map_err(|_| "Invalid migration state")?;
    if record.id != id {
        return Err("Migration state identity mismatch".into());
    }
    Ok(Some(record))
}
fn save(files: &FileSet, record: &Record) -> Result<(), String> {
    let text = serde_json::to_string(record).map_err(|_| "Invalid migration state")?;
    if text.len() > 1024 * 1024 {
        return Err("Migration state exceeds size limit".into());
    }
    files.commit(vec![Change {
        path: filename(&record.id)?,
        contents: Some(text),
    }])
}
trait Remote {
    fn pair(&self, id: &str, invitation: &str, proof: &str, local: &str) -> Result<(), String>;
    fn status(&self, id: &str, proof: &str) -> Result<MigrationSessionView, String>;
}
impl Remote for Client {
    fn pair(&self, id: &str, invitation: &str, proof: &str, local: &str) -> Result<(), String> {
        Client::pair(self, id, invitation, proof, local)
    }
    fn status(&self, id: &str, proof: &str) -> Result<MigrationSessionView, String> {
        Client::status(self, id, proof)
    }
}
fn pin(record: &mut Record, view: MigrationSessionView) -> Result<(), String> {
    if view.id != record.id
        || view.local_environment.as_deref() != Some(&record.local_environment)
        || view.hosted_environment == record.local_environment
        || view.hosted_environment.is_empty()
        || view.owner_id.is_empty()
    {
        return Err("Paired migration environment does not match".into());
    }
    if record.view.as_ref().is_some_and(|previous| {
        previous.owner_id != view.owner_id
            || previous.direction != view.direction
            || previous.hosted_environment != view.hosted_environment
    }) {
        return Err("Paired migration owner or environment changed".into());
    }
    record.view = Some(view);
    Ok(())
}
fn pair_at(
    root: &Path,
    origin: &str,
    id: &str,
    invitation: &str,
    local: &str,
    remote: &impl Remote,
) -> Result<MigrationSessionView, String> {
    filename(id)?;
    let files = FileSet::acquire_wait(root)?;
    let mut record = match load(root, id)? {
        Some(record) if record.origin == origin && record.local_environment == local => record,
        Some(_) => return Err("Migration session belongs to another origin or environment".into()),
        None => Record {
            id: id.into(),
            origin: origin.into(),
            proof: fabrials_runtime::migration::client::new_proof()?,
            local_environment: local.into(),
            view: None,
            transfer: None,
            imported: None,
            oauth_generations: Default::default(),
        },
    };
    save(&files, &record)?;
    let view = match remote.status(id, &record.proof) {
        Ok(view) => view,
        Err(_) => {
            let paired = remote.pair(id, invitation, &record.proof, local);
            match remote.status(id, &record.proof) {
                Ok(view) => view,
                Err(error) => return Err(paired.err().unwrap_or(error)),
            }
        }
    };
    pin(&mut record, view)?;
    save(&files, &record)?;
    record.view.ok_or("Migration pairing unavailable".into())
}
pub fn pair(origin: &str, id: &str, invitation: &str) -> Result<MigrationSessionView, String> {
    let client = Client::new(origin)?;
    let local = crate::local_control::environment_id()?;
    pair_at(&root(), client.origin(), id, invitation, &local, &client)
}
pub fn status(id: &str) -> Result<MigrationSessionView, String> {
    let directory = root();
    let files = FileSet::acquire_wait(&directory)?;
    let mut record = load(&directory, id)?.ok_or("Migration session not found")?;
    if record.local_environment != crate::local_control::environment_id()? {
        return Err("Local migration environment changed".into());
    }
    let view = Client::new(&record.origin)?.status(id, &record.proof)?;
    pin(&mut record, view)?;
    save(&files, &record)?;
    record.view.ok_or("Migration session unavailable".into())
}
pub fn inventory(id: &str) -> Result<Vec<super::MigrationCandidate>, String> {
    let directory = root();
    let _files = FileSet::acquire_wait(&directory)?;
    let record = load(&directory, id)?.ok_or("Migration session not found")?;
    let client = Client::new(&record.origin)?;
    let mut record = record;
    let current = client.status(id, &record.proof)?;
    pin(&mut record, current)?;
    match record
        .view
        .as_ref()
        .ok_or("Migration session unavailable")?
        .direction
    {
        fabrials_core::migration::MigrationDirection::LocalToHosted => super::candidates(),
        fabrials_core::migration::MigrationDirection::HostedToLocal => {
            client.candidates(id, &record.proof)
        }
    }
}

pub fn propose(
    id: &str,
    selection: &[super::MigrationSelection],
) -> Result<MigrationSessionView, String> {
    use fabrials_core::migration::{destination_providers, review, MigrationDirection};
    let directory = root();
    let files = FileSet::acquire_wait(&directory)?;
    let mut record = load(&directory, id)?.ok_or("Migration session not found")?;
    if record.local_environment != crate::local_control::environment_id()? {
        return Err("Local migration environment changed".into());
    }
    let client = Client::new(&record.origin)?;
    let current = client.status(id, &record.proof)?;
    pin(&mut record, current)?;
    let view = record
        .view
        .as_ref()
        .ok_or("Migration session unavailable")?;
    if view.phase != "paired" {
        return Err("Create a new migration to change an existing review".into());
    }
    let local = super::candidates()?;
    let hosted = client.candidates(id, &record.proof)?;
    let (source, destination, candidates, occupied) = match view.direction {
        MigrationDirection::LocalToHosted => (
            &record.local_environment,
            &view.hosted_environment,
            &local,
            &hosted,
        ),
        MigrationDirection::HostedToLocal => (
            &view.hosted_environment,
            &record.local_environment,
            &hosted,
            &local,
        ),
    };
    let occupied = occupied
        .iter()
        .map(|account| account.id.clone())
        .collect::<Vec<_>>();
    let review = review(
        source,
        destination,
        candidates,
        selection,
        &destination_providers(),
        &occupied,
    )?;
    client.propose(id, &record.proof, &review)?;
    let current = client.status(id, &record.proof)?;
    if current.review.as_ref() != Some(&review) {
        return Err("Hosted migration review does not match".into());
    }
    pin(&mut record, current)?;
    save(&files, &record)?;
    record.view.ok_or("Migration review unavailable".into())
}

/// The renderer confirms only the revision it displayed, never key material.
pub fn execute(id: &str, confirmed_revision: &str) -> Result<Vec<String>, String> {
    let directory = root();
    let files = FileSet::acquire_wait(&directory)?;
    let record = load(&directory, id)?.ok_or("Migration session not found")?;
    if record.local_environment != crate::local_control::environment_id()? {
        return Err("Local migration environment changed".into());
    }
    let client = Client::new(&record.origin)?;
    Execution {
        files: &files,
        accounts: &crate::app::data_dir().join("accounts"),
        client: &client,
        #[cfg(test)]
        exit_before_receipt: false,
    }
    .run(record, confirmed_revision)
}

trait TransferRemote {
    fn status(&self, id: &str, proof: &str) -> Result<MigrationSessionView, String>;
    fn export(
        &self,
        id: &str,
        proof: &str,
        revision: &str,
        review: &fabrials_core::migration::MigrationReview,
    ) -> Result<Vec<fabrials_accounts::transfer::ApiKeyTransfer>, String>;
    fn import(
        &self,
        id: &str,
        proof: &str,
        revision: &str,
        entries: &[fabrials_accounts::transfer::ApiKeyTransfer],
    ) -> Result<Vec<String>, String>;
    fn acknowledge_export(&self, id: &str, proof: &str, revision: &str) -> Result<(), String>;
}
impl TransferRemote for Client {
    fn status(&self, id: &str, proof: &str) -> Result<MigrationSessionView, String> {
        Client::status(self, id, proof)
    }
    fn export(
        &self,
        id: &str,
        proof: &str,
        revision: &str,
        review: &fabrials_core::migration::MigrationReview,
    ) -> Result<Vec<fabrials_accounts::transfer::ApiKeyTransfer>, String> {
        Client::export(self, id, proof, revision, review)
    }
    fn import(
        &self,
        id: &str,
        proof: &str,
        revision: &str,
        entries: &[fabrials_accounts::transfer::ApiKeyTransfer],
    ) -> Result<Vec<String>, String> {
        Client::import(self, id, proof, revision, entries)
    }
    fn acknowledge_export(&self, id: &str, proof: &str, revision: &str) -> Result<(), String> {
        Client::acknowledge_export(self, id, proof, revision)
    }
}
struct Execution<'a> {
    files: &'a FileSet,
    accounts: &'a Path,
    client: &'a dyn TransferRemote,
    #[cfg(test)]
    exit_before_receipt: bool,
}
impl Execution<'_> {
    fn run(&self, mut record: Record, confirmed_revision: &str) -> Result<Vec<String>, String> {
        use fabrials_core::migration::MigrationDirection;
        let id = record.id.clone();
        let id = id.as_str();
        let client = self.client;
        let files = self.files;
        let view = client.status(id, &record.proof)?;
        pin(&mut record, view)?;
        let view = record
            .view
            .as_ref()
            .ok_or("Migration session unavailable")?;
        if view.revision.as_deref() != Some(confirmed_revision)
            || !matches!(view.phase.as_str(), "approved" | "completed")
        {
            return Err("Confirm the current approved migration review".into());
        }
        let review = view
            .review
            .as_ref()
            .ok_or("Migration review unavailable")?
            .clone();
        use sha2::{Digest, Sha256};
        let actual_revision = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&review).map_err(|_| "Invalid migration review")?)
        );
        if actual_revision != confirmed_revision {
            return Err("Migration review revision does not match".into());
        }
        let completed = view.phase == "completed";
        let direction = view.direction;
        if record.imported.is_none() {
            if record.transfer.is_none() {
                record.transfer = Some(match direction {
                    MigrationDirection::LocalToHosted => {
                        super::prepare_transfer(&record.local_environment, &review)?
                    }
                    MigrationDirection::HostedToLocal => {
                        client.export(id, &record.proof, confirmed_revision, &review)?
                    }
                });
                // Persist the exact batch before crossing either commit boundary.
                save(files, &record)?;
            }
            let entries = record
                .transfer
                .as_ref()
                .ok_or("Migration transfer unavailable")?;
            let ids = match direction {
                MigrationDirection::LocalToHosted => {
                    if !completed {
                        let current = super::prepare_transfer(&record.local_environment, &review)?;
                        if current != *entries {
                            return Err("Migration source changed; create a new review".into());
                        }
                    }
                    client.import(id, &record.proof, confirmed_revision, entries)?
                }
                MigrationDirection::HostedToLocal => fabrials_runtime::migration::import_files(
                    self.accounts,
                    &record.local_environment,
                    id,
                    &review,
                    entries,
                )?,
            };
            #[cfg(test)]
            if self.exit_before_receipt {
                std::process::exit(75);
            }
            record.imported = Some(ids);
            record.transfer = None;
            save(files, &record)?;
        }
        if direction == MigrationDirection::HostedToLocal {
            client.acknowledge_export(id, &record.proof, confirmed_revision)?;
        }
        if let Some(view) = record.view.as_mut() {
            view.phase = "completed".into();
        }
        save(files, &record)?;
        record
            .imported
            .ok_or("Migration receipt unavailable".into())
    }
}

/// Forget local receipt metadata only; account and import-deduplication stores remain intact.
pub fn forget(id: &str) -> Result<(), String> {
    forget_at(&root(), id, crate::util::now_ms())
}
fn forget_at(directory: &Path, id: &str, now: i64) -> Result<(), String> {
    let files = FileSet::acquire_wait(directory)?;
    let record = load(directory, id)?.ok_or("Migration receipt unavailable")?;
    let view = record
        .view
        .as_ref()
        .ok_or("Migration receipt unavailable")?;
    if record.imported.is_none() || record.transfer.is_some() {
        return Err("Finish or cancel the pending migration first".into());
    }
    if view.phase != "completed" && now < view.expires_at_ms {
        return Err("Recover the transfer receipt before forgetting it".into());
    }
    files.commit(vec![Change {
        path: filename(id)?,
        contents: None,
    }])
}

pub fn cancel(id: &str) -> Result<(), String> {
    let directory = root();
    let files = FileSet::acquire_wait(&directory)?;
    let record = load(&directory, id)?.ok_or("Migration session not found")?;
    Client::new(&record.origin)?.cancel(id, &record.proof)?;
    files.commit(vec![Change {
        path: filename(id)?,
        contents: None,
    }])
}

fn oauth_item(
    record: &Record,
    source_id: &str,
) -> Result<fabrials_core::migration::MigrationItem, String> {
    use fabrials_core::migration::{MigrationAction, MigrationDirection};
    let view = record.view.as_ref().ok_or("Migration review unavailable")?;
    if record.imported.is_none() || view.direction != MigrationDirection::HostedToLocal {
        return Err("Complete the transfer to this installation first".into());
    }
    view.review
        .as_ref()
        .and_then(|review| {
            review.items.iter().find(|item| {
                item.source_id == source_id && item.action == MigrationAction::AuthorizeOAuth
            })
        })
        .cloned()
        .ok_or("Authorization is not part of this migration".into())
}

pub fn begin_authorization(
    id: &str,
    source_id: &str,
) -> Result<crate::account_login::LoginView, String> {
    let local = crate::local_control::environment_id()?;
    let directory = root();
    let item = {
        let _files = FileSet::acquire_wait(&directory)?;
        let record = load(&directory, id)?.ok_or("Migration unavailable")?;
        if record.local_environment != local {
            return Err("Migration belongs to another installation".into());
        }
        oauth_item(&record, source_id)?
    };
    crate::account_login::begin_reviewed(
        &item.provider,
        item.target_alias.clone(),
        false,
        false,
        |account| {
            let files = FileSet::acquire_wait(&directory)?;
            let mut record = load(&directory, id)?.ok_or("Migration unavailable")?;
            if record.local_environment != local || oauth_item(&record, source_id)? != item {
                return Err("Migration review changed".into());
            }
            record.oauth_generations.insert(
                account.id.clone(),
                account
                    .generation
                    .clone()
                    .ok_or("Account identity unavailable")?,
            );
            save(&files, &record)
        },
    )
}

fn matching_authorizations(record: &Record, accounts: &[crate::accounts::Account]) -> Vec<String> {
    record
        .oauth_generations
        .iter()
        .filter(|(id, generation)| {
            accounts.iter().any(|account| {
                &account.id == *id && account.generation.as_ref() == Some(*generation)
            })
        })
        .map(|(id, _)| id.clone())
        .collect()
}

/// Read only local identities; never infer completion from an alias alone.
pub fn authorizations(id: &str) -> Result<Vec<String>, String> {
    let accounts = crate::accounts::routing_registry()?.accounts;
    let directory = root();
    let _files = FileSet::acquire_wait(&directory)?;
    let record = load(&directory, id)?.ok_or("Migration unavailable")?;
    Ok(matching_authorizations(&record, &accounts))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    #[test]
    #[ignore = "Requires the isolated HTTPS fixture and a fresh native invitation"]
    fn native_https_pairing_lifecycle() {
        let directory =
            PathBuf::from(std::env::var_os("FABRIALS_HTTPS_FIXTURE_ROOT").expect("fixture root"));
        assert!(directory.starts_with(std::env::temp_dir()));
        assert!(directory
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("fabrials-https-"));
        let origin = std::env::var("FABRIALS_HTTPS_FIXTURE_ORIGIN").unwrap();
        assert_eq!(origin, "https://localhost:19488");
        let invitation: serde_json::Value =
            serde_json::from_slice(&std::fs::read(directory.join("gui-invitation.json")).unwrap())
                .unwrap();
        let id = invitation["id"].as_str().unwrap();
        eprintln!("Native pairing started");
        let view = pair(&origin, id, invitation["secret"].as_str().unwrap()).unwrap();
        assert_eq!(view.phase, "paired");
        eprintln!("Native pairing persisted");
        assert_eq!(status(id).unwrap().phase, "paired");
        assert!(saved()
            .unwrap()
            .iter()
            .any(|entry| entry.id == id && entry.view.is_some()));
        cancel(id).unwrap();
        assert!(status(id).is_err());
        assert!(!saved().unwrap().iter().any(|entry| entry.id == id));
    }

    struct Fixture {
        root: PathBuf,
        paired: Cell<bool>,
        calls: Cell<usize>,
        owner: RefCell<String>,
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
    impl Remote for Fixture {
        fn pair(&self, id: &str, _: &str, proof: &str, local: &str) -> Result<(), String> {
            let persisted = load(&self.root, id)?.unwrap();
            assert_eq!(persisted.proof, proof);
            assert_eq!(persisted.local_environment, local);
            self.calls.set(self.calls.get() + 1);
            self.paired.set(true);
            Err("Response lost after server consumed invitation".into())
        }
        fn status(&self, id: &str, proof: &str) -> Result<MigrationSessionView, String> {
            if !self.paired.get() {
                return Err("Not paired".into());
            }
            assert_eq!(load(&self.root, id)?.unwrap().proof, proof);
            Ok(MigrationSessionView {
                id: id.into(),
                owner_id: self.owner.borrow().clone(),
                direction: fabrials_core::migration::MigrationDirection::LocalToHosted,
                hosted_environment: "hosted-fixture".into(),
                local_environment: Some("local-fixture".into()),
                phase: "paired".into(),
                review: None,
                revision: None,
                expires_at_ms: i64::MAX,
            })
        }
    }
    fn fixture() -> Fixture {
        Fixture {
            root: std::env::temp_dir().join(format!(
                "spanreed-pair-{}",
                fabrials_runtime::migration::client::new_proof().unwrap()
            )),
            paired: Cell::new(false),
            calls: Cell::new(0),
            owner: RefCell::new("owner-fixture".into()),
        }
    }
    const ID: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    fn connect(f: &Fixture) -> Result<MigrationSessionView, String> {
        pair_at(
            &f.root,
            "https://relay.example",
            ID,
            "invitation",
            "local-fixture",
            f,
        )
    }
    #[test]
    fn forgetting_requires_a_receipt_and_preserves_other_files() {
        let f = fixture();
        connect(&f).unwrap();
        assert!(forget_at(&f.root, ID, 0).is_err());
        let files = FileSet::acquire_wait(&f.root).unwrap();
        let mut record = load(&f.root, ID).unwrap().unwrap();
        record.imported = Some(vec![]);
        save(&files, &record).unwrap();
        drop(files);
        assert!(forget_at(&f.root, ID, 0).is_err());
        let files = FileSet::acquire_wait(&f.root).unwrap();
        record.view.as_mut().unwrap().phase = "completed".into();
        save(&files, &record).unwrap();
        drop(files);
        std::fs::write(f.root.join("unrelated"), "keep").unwrap();
        forget_at(&f.root, ID, 0).unwrap();
        assert!(load(&f.root, ID).unwrap().is_none());
        assert_eq!(
            std::fs::read_to_string(f.root.join("unrelated")).unwrap(),
            "keep"
        );
        assert!(forget_at(&f.root, ID, 0).is_err());
        let files = FileSet::acquire_wait(&f.root).unwrap();
        let view = record.view.as_mut().unwrap();
        view.phase = "approved".into();
        view.expires_at_ms = 10;
        save(&files, &record).unwrap();
        drop(files);
        assert!(forget_at(&f.root, ID, 9).is_err());
        forget_at(&f.root, ID, 10).unwrap();
    }

    #[test]
    fn oauth_receipt_survives_reload_and_requires_exact_account_generation() {
        use fabrials_core::migration::{
            MigrationAction, MigrationDirection, MigrationItem, MigrationReview,
        };
        let f = fixture();
        connect(&f).unwrap();
        let files = FileSet::acquire_wait(&f.root).unwrap();
        let mut record = load(&f.root, ID).unwrap().unwrap();
        let item = MigrationItem {
            source_id: "grok/source".into(),
            source_generation: "source-generation".into(),
            provider: "grok".into(),
            target_alias: "destination".into(),
            action: MigrationAction::AuthorizeOAuth,
        };
        let view = record.view.as_mut().unwrap();
        view.direction = MigrationDirection::HostedToLocal;
        view.review = Some(MigrationReview {
            source_environment: "hosted-fixture".into(),
            destination_environment: "local-fixture".into(),
            items: vec![item.clone()],
        });
        assert!(oauth_item(&record, &item.source_id).is_err());
        record.imported = Some(vec![]);
        assert_eq!(oauth_item(&record, &item.source_id).unwrap(), item);
        assert!(oauth_item(&record, "grok/unreviewed").is_err());
        let mut account = crate::accounts::Account::new("grok", "destination").unwrap();
        record
            .oauth_generations
            .insert(account.id.clone(), account.generation.clone().unwrap());
        save(&files, &record).unwrap();
        drop(files);
        let resumed = load(&f.root, ID).unwrap().unwrap();
        assert!(matching_authorizations(&resumed, &[]).is_empty());
        assert_eq!(
            matching_authorizations(&resumed, &[account.clone()]),
            vec![account.id.clone()]
        );
        account.generation = Some("unrelated-replacement".into());
        assert!(matching_authorizations(&resumed, &[account]).is_empty());
        record.view.as_mut().unwrap().direction = MigrationDirection::LocalToHosted;
        assert!(oauth_item(&record, &item.source_id).is_err());
    }

    #[test]
    fn persisted_proof_recovers_lost_pair_response_and_restart() {
        let f = fixture();
        assert_eq!(connect(&f).unwrap().phase, "paired");
        let before = load(&f.root, ID).unwrap().unwrap().proof;
        assert_eq!(connect(&f).unwrap().owner_id, "owner-fixture");
        assert_eq!(load(&f.root, ID).unwrap().unwrap().proof, before);
        assert_eq!(f.calls.get(), 1);
        let text = std::fs::read_to_string(f.root.join(filename(ID).unwrap())).unwrap();
        assert!(!text.contains("invitation"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(f.root.join(filename(ID).unwrap()))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }
    #[test]
    fn transfer_state_is_private_and_receipt_removes_cached_keys() {
        use fabrials_accounts::transfer::{ApiKey, ApiKeyTransfer};
        let f = fixture();
        connect(&f).unwrap();
        let files = FileSet::acquire_wait(&f.root).unwrap();
        let mut record = load(&f.root, ID).unwrap().unwrap();
        record.transfer = Some(vec![ApiKeyTransfer {
            source_id: "nous/source".into(),
            source_generation: "generation".into(),
            provider: "nous".into(),
            target_alias: "target".into(),
            api_key: ApiKey::new("synthetic-transfer-key".into()).unwrap(),
        }]);
        save(&files, &record).unwrap();
        let mut resumed = load(&f.root, ID).unwrap().unwrap();
        assert_eq!(
            resumed.transfer.as_ref().unwrap()[0].api_key.expose(),
            "synthetic-transfer-key"
        );
        resumed.imported = Some(vec!["nous/target".into()]);
        resumed.transfer = None;
        save(&files, &resumed).unwrap();
        let raw = std::fs::read_to_string(f.root.join(filename(ID).unwrap())).unwrap();
        assert!(!raw.contains("synthetic-transfer-key"));
        assert_eq!(
            load(&f.root, ID).unwrap().unwrap().imported.unwrap(),
            vec!["nous/target"]
        );
    }
    #[test]
    fn saved_inventory_never_serializes_device_proof_or_cached_keys() {
        let f = fixture();
        connect(&f).unwrap();
        let record = load(&f.root, ID).unwrap().unwrap();
        {
            let files = FileSet::acquire_wait(&f.root).unwrap();
            let mut pending = load(&f.root, ID).unwrap().unwrap();
            pending.transfer = Some(vec![fabrials_accounts::transfer::ApiKeyTransfer {
                source_id: "nous/source".into(),
                source_generation: "generation".into(),
                provider: "nous".into(),
                target_alias: "target".into(),
                api_key: fabrials_accounts::transfer::ApiKey::new("private-pending-key".into())
                    .unwrap(),
            }]);
            save(&files, &pending).unwrap();
        }
        let list = saved_at(&f.root).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, ID);
        let rendered = serde_json::to_string(&list).unwrap();
        assert!(!rendered.contains(&record.proof));
        assert!(!rendered.contains("private-pending-key"));
        assert!(!rendered.contains("transfer"));
        assert!(rendered.contains("owner-fixture"));
    }
    #[test]
    fn committed_import_survives_lost_acknowledgement_without_reexport() {
        execution_recovery(false, false);
    }
    #[test]
    fn committed_import_survives_process_exit() {
        execution_recovery(true, false);
    }
    #[test]
    fn committed_import_before_session_receipt_survives_process_exit() {
        execution_recovery(true, true);
    }
    fn execution_recovery(process_exit: bool, before_receipt: bool) {
        use fabrials_accounts::transfer::{ApiKey, ApiKeyTransfer};
        use fabrials_core::migration::{
            MigrationAction, MigrationDirection, MigrationItem, MigrationReview,
        };
        use sha2::{Digest, Sha256};
        struct Network {
            view: serde_json::Value,
            exports: Cell<usize>,
            acknowledgements: Cell<usize>,
            exit_at_ack: bool,
        }
        impl TransferRemote for Network {
            fn status(&self, _: &str, _: &str) -> Result<MigrationSessionView, String> {
                Ok(serde_json::from_value(self.view.clone()).unwrap())
            }
            fn export(
                &self,
                _: &str,
                _: &str,
                _: &str,
                _: &MigrationReview,
            ) -> Result<Vec<ApiKeyTransfer>, String> {
                self.exports.set(self.exports.get() + 1);
                Ok(vec![ApiKeyTransfer {
                    source_id: "nous/source".into(),
                    source_generation: "generation".into(),
                    provider: "nous".into(),
                    target_alias: "copied".into(),
                    api_key: ApiKey::new("synthetic-transfer".into()).unwrap(),
                }])
            }
            fn import(
                &self,
                _: &str,
                _: &str,
                _: &str,
                _: &[ApiKeyTransfer],
            ) -> Result<Vec<String>, String> {
                panic!("wrong transfer direction")
            }
            fn acknowledge_export(&self, _: &str, _: &str, _: &str) -> Result<(), String> {
                if self.exit_at_ack {
                    std::process::exit(74);
                }
                let attempt = self.acknowledgements.get();
                self.acknowledgements.set(attempt + 1);
                if attempt == 0 {
                    Err("response lost".into())
                } else {
                    Ok(())
                }
            }
        }
        let child_root = if process_exit {
            std::env::var_os("SPANREED_MIGRATION_CRASH_ROOT")
        } else {
            None
        };
        let is_child = child_root.is_some();
        let f = if let Some(root) = child_root {
            let root = PathBuf::from(root);
            assert!(root.starts_with(std::env::temp_dir()));
            assert!(root
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("spanreed-pair-"));
            Fixture {
                root,
                paired: Cell::new(true),
                calls: Cell::new(0),
                owner: RefCell::new("owner-fixture".into()),
            }
        } else {
            let f = fixture();
            connect(&f).unwrap();
            f
        };
        let mut record = load(&f.root, ID).unwrap().unwrap();
        let review = MigrationReview {
            source_environment: "hosted-fixture".into(),
            destination_environment: "local-fixture".into(),
            items: vec![MigrationItem {
                source_id: "nous/source".into(),
                source_generation: "generation".into(),
                provider: "nous".into(),
                target_alias: "copied".into(),
                action: MigrationAction::CopyApiKey,
            }],
        };
        let revision = format!("{:x}", Sha256::digest(serde_json::to_vec(&review).unwrap()));
        let view = record.view.as_mut().unwrap();
        view.direction = MigrationDirection::HostedToLocal;
        view.phase = "approved".into();
        view.review = Some(review);
        view.revision = Some(revision.clone());
        let network = Network {
            view: serde_json::to_value(view).unwrap(),
            exports: Cell::new(0),
            acknowledgements: Cell::new(if process_exit && !is_child { 1 } else { 0 }),
            exit_at_ack: is_child,
        };
        let accounts = f.root.join("vault");
        {
            let files = FileSet::acquire_wait(&f.root).unwrap();
            save(&files, &record).unwrap();
            if !process_exit || is_child {
                let execution = Execution {
                    files: &files,
                    accounts: &accounts,
                    client: &network,
                    exit_before_receipt: is_child && before_receipt,
                };
                assert_eq!(
                    execution.run(record, &revision).unwrap_err(),
                    "response lost"
                );
            }
        }
        if process_exit && !is_child {
            let child = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    if before_receipt {
                        "migration::session::tests::committed_import_before_session_receipt_survives_process_exit"
                    } else {
                        "migration::session::tests::committed_import_survives_process_exit"
                    },
                ])
                .env("SPANREED_MIGRATION_CRASH_ROOT", &f.root)
                .output()
                .unwrap();
            assert_eq!(
                child.status.code(),
                Some(if before_receipt { 75 } else { 74 }),
                "child did not reach durable checkpoint: {}",
                String::from_utf8_lossy(&child.stderr)
            );
        }
        let resumed = load(&f.root, ID).unwrap().unwrap();
        if before_receipt {
            assert!(resumed.transfer.is_some());
            assert!(resumed.imported.is_none());
        } else {
            assert!(resumed.transfer.is_none());
            assert_eq!(resumed.imported.as_ref().unwrap(), &vec!["nous/copied"]);
        }
        let index_before = std::fs::read(accounts.join("index.json")).unwrap();
        {
            let files = FileSet::acquire_wait(&f.root).unwrap();
            assert_eq!(
                Execution {
                    files: &files,
                    accounts: &accounts,
                    client: &network,
                    exit_before_receipt: false,
                }
                .run(resumed, &revision)
                .unwrap(),
                vec!["nous/copied"]
            );
        }
        let receipt = load(&f.root, ID).unwrap().unwrap();
        assert!(receipt.transfer.is_none());
        assert_eq!(receipt.imported.unwrap(), vec!["nous/copied"]);
        assert_eq!(network.exports.get(), if process_exit { 0 } else { 1 });
        assert_eq!(network.acknowledgements.get(), 2);
        assert_eq!(
            std::fs::read(accounts.join("index.json")).unwrap(),
            index_before
        );
    }
    #[test]
    fn pinned_identity_cannot_be_rebound() {
        let f = fixture();
        connect(&f).unwrap();
        assert!(pair_at(
            &f.root,
            "https://other.example",
            ID,
            "invitation",
            "local-fixture",
            &f
        )
        .is_err());
        assert!(pair_at(
            &f.root,
            "https://relay.example",
            ID,
            "invitation",
            "other-local",
            &f
        )
        .is_err());
        *f.owner.borrow_mut() = "another-owner".into();
        assert!(connect(&f).unwrap_err().contains("owner"));
        assert_eq!(
            load(&f.root, ID).unwrap().unwrap().view.unwrap().owner_id,
            "owner-fixture"
        );
        assert_eq!(f.calls.get(), 1);
    }
}
