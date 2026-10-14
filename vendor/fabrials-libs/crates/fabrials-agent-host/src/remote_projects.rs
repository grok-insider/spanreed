use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cap_std::fs::Dir;
use serde::{Deserialize, Serialize};

use crate::workspace::{FilesystemIdentity, WorkspaceError, WorkspaceIndex};

const ROOTS_FILE: &str = "remote-roots.json";
const MAX_ROOTS: usize = 32;
const MAX_HANDLES: usize = 1024;
const MAX_DIRECTORY_ENTRIES: usize = 4096;
const PAGE_SIZE: usize = 100;
const HANDLE_TTL_MS: u64 = 300_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteRoot {
    pub root_id: String,
    pub label: String,
    pub canonical_path: PathBuf,
    pub filesystem_identity: FilesystemIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteDirectory {
    pub directory_id: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteProjectPage {
    pub parent_id: Option<String>,
    pub entries: Vec<RemoteDirectory>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone)]
struct Handle {
    principal: String,
    root_id: String,
    relative: PathBuf,
    identity: FilesystemIdentity,
    expires_at: u64,
}

#[derive(Debug)]
struct PageCursor {
    principal: String,
    directory_id: String,
    entries: Vec<RemoteDirectory>,
    next_cursor: Option<String>,
    expires_at: u64,
}

#[derive(Debug, Default)]
pub(crate) struct RemoteProjects {
    roots: BTreeMap<String, RemoteRoot>,
    handles: BTreeMap<String, Handle>,
    cursors: BTreeMap<String, PageCursor>,
}

fn opaque() -> Result<String, &'static str> {
    crate::pairing::random_hex()
        .map(|value| value.expose().to_owned())
        .map_err(|_| "entropy_unavailable")
}

fn label(path: &Path) -> String {
    crate::workspace::display_name_for(path)
}

#[cfg(unix)]
fn identity(directory: &Dir) -> Result<FilesystemIdentity, &'static str> {
    use cap_std::fs::MetadataExt as _;
    let metadata = directory
        .dir_metadata()
        .map_err(|_| "remote_directory_unavailable")?;
    Ok(FilesystemIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

#[cfg(windows)]
fn identity(directory: &Dir) -> Result<FilesystemIdentity, &'static str> {
    crate::win_fs::identity(directory).map_err(|_| "remote_directory_unavailable")
}

fn is_reparse(metadata: &cap_std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use cap_std::fs::MetadataExt;
        metadata.file_attributes()
            & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT
            != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

#[cfg(not(any(unix, windows)))]
fn identity(_: &Dir) -> Result<FilesystemIdentity, &'static str> {
    Err("remote_projects_unsupported")
}

impl RemoteProjects {
    pub(crate) fn load(directory: &Path) -> Result<Self, &'static str> {
        let path = directory.join(ROOTS_FILE);
        use std::io::Read as _;
        let bytes = match std::fs::File::open(path).and_then(|file| {
            let mut bytes = Vec::new();
            file.take(128 * 1024 + 1).read_to_end(&mut bytes)?;
            Ok(bytes)
        }) {
            Ok(bytes) if bytes.len() <= 128 * 1024 => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            _ => return Err("remote_roots_unusable"),
        };
        let roots: Vec<RemoteRoot> =
            serde_json::from_slice(&bytes).map_err(|_| "remote_roots_unusable")?;
        if roots.len() > MAX_ROOTS
            || roots
                .iter()
                .any(|root| root.root_id.is_empty() || !root.canonical_path.is_absolute())
        {
            return Err("remote_roots_unusable");
        }
        let roots: BTreeMap<_, _> = roots
            .into_iter()
            .map(|root| (root.root_id.clone(), root))
            .collect();
        Ok(Self {
            roots,
            ..Self::default()
        })
    }

    pub(crate) fn roots(&self) -> Vec<RemoteRoot> {
        self.roots.values().cloned().collect()
    }

    fn persist(&self, directory: &Path) -> Result<(), &'static str> {
        use std::io::Write as _;
        let temporary = directory.join(format!("{ROOTS_FILE}.{}.tmp", opaque()?));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let result = (|| {
            let mut file = options
                .open(&temporary)
                .map_err(|_| "remote_roots_unusable")?;
            let bytes = serde_json::to_vec(&self.roots()).map_err(|_| "remote_roots_unusable")?;
            file.write_all(&bytes)
                .map_err(|_| "remote_roots_unusable")?;
            file.sync_all().map_err(|_| "remote_roots_unusable")?;
            std::fs::rename(&temporary, directory.join(ROOTS_FILE))
                .map_err(|_| "remote_roots_unusable")?;
            #[cfg(unix)]
            std::fs::File::open(directory)
                .and_then(|file| file.sync_all())
                .map_err(|_| "remote_roots_unusable")?;
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(temporary);
        }
        result
    }

    pub(crate) fn authorize(
        &mut self,
        state_directory: &Path,
        selected: &Path,
    ) -> Result<Vec<RemoteRoot>, &'static str> {
        if !cfg!(any(unix, windows)) {
            return Err("remote_projects_unsupported");
        }
        let canonical_path = selected
            .canonicalize()
            .map_err(|_| "remote_directory_unavailable")?;
        let opened = Dir::open_ambient_dir(&canonical_path, cap_std::ambient_authority())
            .map_err(|_| "remote_directory_unavailable")?;
        let filesystem_identity = identity(&opened)?;
        if self
            .roots
            .values()
            .any(|root| root.filesystem_identity == filesystem_identity)
        {
            return Ok(self.roots());
        }
        if self.roots.len() >= MAX_ROOTS {
            return Err("remote_root_limit");
        }
        let root_id = opaque()?;
        self.roots.insert(
            root_id.clone(),
            RemoteRoot {
                root_id: root_id.clone(),
                label: label(&canonical_path),
                canonical_path,
                filesystem_identity,
            },
        );
        if let Err(error) = self.persist(state_directory) {
            self.roots.remove(&root_id);
            return Err(error);
        }
        Ok(self.roots())
    }

    pub(crate) fn revoke(
        &mut self,
        directory: &Path,
        root_id: &str,
    ) -> Result<Vec<RemoteRoot>, &'static str> {
        let old = self.roots.remove(root_id).ok_or("unknown_remote_root")?;
        if let Err(error) = self.persist(directory) {
            self.roots.insert(root_id.into(), old);
            return Err(error);
        }
        self.handles.retain(|_, value| value.root_id != root_id);
        self.cursors.clear();
        Ok(self.roots())
    }

    fn purge(&mut self, now: u64) {
        self.handles.retain(|_, value| value.expires_at > now);
        self.cursors.retain(|_, value| value.expires_at > now);
    }

    fn handle(
        &mut self,
        principal: &str,
        root: &RemoteRoot,
        relative: PathBuf,
        identity: FilesystemIdentity,
        now: u64,
    ) -> Result<String, &'static str> {
        if let Some((id, handle)) = self.handles.iter_mut().find(|(_, handle)| {
            handle.principal == principal
                && handle.root_id == root.root_id
                && handle.relative == relative
                && handle.identity == identity
        }) {
            handle.expires_at = now.saturating_add(HANDLE_TTL_MS);
            return Ok(id.clone());
        }
        if self.handles.len() >= MAX_HANDLES {
            return Err("remote_directory_handle_limit");
        }
        let id = opaque()?;
        self.handles.insert(
            id.clone(),
            Handle {
                principal: principal.into(),
                root_id: root.root_id.clone(),
                relative,
                identity,
                expires_at: now.saturating_add(HANDLE_TTL_MS),
            },
        );
        Ok(id)
    }

    fn opened(
        &self,
        principal: &str,
        id: &str,
        now: u64,
    ) -> Result<(Dir, PathBuf, Handle), &'static str> {
        let handle = self
            .handles
            .get(id)
            .filter(|handle| handle.principal == principal && handle.expires_at > now)
            .ok_or("unknown_remote_directory")?;
        let root = self
            .roots
            .get(&handle.root_id)
            .ok_or("unknown_remote_root")?;
        let root_meta = std::fs::symlink_metadata(&root.canonical_path)
            .map_err(|_| "remote_directory_unavailable")?;
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if root_meta.file_attributes()
                & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT
                != 0
            {
                return Err("remote_directory_changed");
            }
        }
        if root_meta.file_type().is_symlink() {
            return Err("remote_directory_changed");
        }
        let mut opened = Dir::open_ambient_dir(&root.canonical_path, cap_std::ambient_authority())
            .map_err(|_| "remote_directory_unavailable")?;
        if identity(&opened)? != root.filesystem_identity {
            return Err("remote_directory_changed");
        }
        #[cfg(windows)]
        let root_final_path =
            crate::win_fs::final_path(&opened).map_err(|_| "remote_directory_changed")?;
        for component in handle.relative.components() {
            let std::path::Component::Normal(name) = component else {
                return Err("remote_directory_changed");
            };
            let meta = opened
                .symlink_metadata(name)
                .map_err(|_| "remote_directory_unavailable")?;
            if !meta.is_dir() || is_reparse(&meta) {
                return Err("remote_directory_changed");
            }
            opened = opened
                .open_dir(name)
                .map_err(|_| "remote_directory_unavailable")?;
        }
        if identity(&opened)? != handle.identity {
            return Err("remote_directory_changed");
        }
        #[cfg(windows)]
        let path = crate::win_fs::confined_path(&root_final_path, &opened)
            .map_err(|_| "remote_directory_changed")?;
        #[cfg(not(windows))]
        let path = root.canonical_path.join(&handle.relative);
        Ok((opened, path, handle.clone()))
    }

    pub(crate) fn list(
        &mut self,
        principal: &str,
        directory_id: Option<&str>,
        cursor: Option<&str>,
        now: u64,
    ) -> Result<RemoteProjectPage, &'static str> {
        self.purge(now);
        if !cfg!(any(unix, windows)) {
            return Err("remote_projects_unsupported");
        }
        if let Some(cursor) = cursor {
            let page = self
                .cursors
                .get(cursor)
                .filter(|page| {
                    page.principal == principal && directory_id == Some(page.directory_id.as_str())
                })
                .ok_or("unknown_remote_cursor")?;
            self.opened(principal, &page.directory_id, now)?;
            return Ok(RemoteProjectPage {
                parent_id: directory_id.map(str::to_owned),
                entries: page.entries.clone(),
                next_cursor: page.next_cursor.clone(),
            });
        }
        let Some(id) = directory_id else {
            let mut entries = Vec::new();
            for root in self.roots() {
                let directory_id = self.handle(
                    principal,
                    &root,
                    PathBuf::new(),
                    root.filesystem_identity,
                    now,
                )?;
                if self.opened(principal, &directory_id, now).is_ok() {
                    entries.push(RemoteDirectory {
                        directory_id,
                        label: label(&root.canonical_path),
                    });
                } else {
                    self.handles.remove(&directory_id);
                }
            }
            return Ok(RemoteProjectPage {
                parent_id: None,
                entries,
                next_cursor: None,
            });
        };
        let (opened, _, handle) = self.opened(principal, id, now)?;
        let root = self
            .roots
            .get(&handle.root_id)
            .cloned()
            .ok_or("unknown_remote_root")?;
        let mut children = Vec::new();
        for (count, entry) in opened
            .entries()
            .map_err(|_| "remote_directory_unavailable")?
            .enumerate()
        {
            if count >= MAX_DIRECTORY_ENTRIES {
                return Err("remote_directory_too_large");
            }
            let entry = entry.map_err(|_| "remote_directory_unavailable")?;
            let name = entry.file_name();
            let meta = opened
                .symlink_metadata(&name)
                .map_err(|_| "remote_directory_changed")?;
            if !meta.is_dir() || is_reparse(&meta) {
                continue;
            }
            let child = opened
                .open_dir(&name)
                .map_err(|_| "remote_directory_changed")?;
            let child_identity = identity(&child)?;
            #[cfg(windows)]
            {
                let parent_path =
                    crate::win_fs::final_path(&opened).map_err(|_| "remote_directory_changed")?;
                crate::win_fs::confined_path(&parent_path, &child)
                    .map_err(|_| "remote_directory_changed")?;
            }
            children.push((handle.relative.join(name), child_identity));
        }
        let new_handles = children
            .iter()
            .filter(|(relative, identity)| {
                !self.handles.values().any(|existing| {
                    existing.principal == principal
                        && existing.root_id == root.root_id
                        && existing.relative == *relative
                        && existing.identity == *identity
                })
            })
            .count();
        if self.handles.len().saturating_add(new_handles) > MAX_HANDLES {
            return Err("remote_directory_handle_limit");
        }
        children.sort_by(|a, b| a.0.cmp(&b.0));
        let mut entries = Vec::with_capacity(children.len());
        for (relative, child_identity) in children {
            entries.push(RemoteDirectory {
                label: label(&relative),
                directory_id: self.handle(principal, &root, relative, child_identity, now)?,
            });
        }
        let next_cursor = if entries.len() > PAGE_SIZE {
            let remaining = entries.split_off(PAGE_SIZE);
            let pages: Vec<_> = remaining.chunks(PAGE_SIZE).collect();
            let mut next_cursor: Option<String> = None;
            for page in pages.iter().rev() {
                let existing = self.cursors.iter_mut().find(|(_, cursor)| {
                    cursor.principal == principal
                        && cursor.directory_id == id
                        && cursor.entries == *page
                        && cursor.next_cursor == next_cursor
                });
                let cursor_id = if let Some((cursor_id, cursor)) = existing {
                    cursor.expires_at = now.saturating_add(HANDLE_TTL_MS);
                    cursor_id.clone()
                } else {
                    if self.cursors.len() >= 64 {
                        return Err("remote_directory_cursor_limit");
                    }
                    let cursor_id = opaque()?;
                    self.cursors.insert(
                        cursor_id.clone(),
                        PageCursor {
                            principal: principal.into(),
                            directory_id: id.into(),
                            entries: page.to_vec(),
                            next_cursor,
                            expires_at: now.saturating_add(HANDLE_TTL_MS),
                        },
                    );
                    cursor_id
                };
                next_cursor = Some(cursor_id);
            }
            next_cursor
        } else {
            None
        };
        Ok(RemoteProjectPage {
            parent_id: Some(id.into()),
            entries,
            next_cursor,
        })
    }

    pub(crate) fn enrol(
        &self,
        principal: &str,
        directory_id: &str,
        now: u64,
        index: &mut WorkspaceIndex,
    ) -> Result<(), &'static str> {
        let (_opened, path, handle) = self.opened(principal, directory_id, now)?;
        index
            .enrol_verified(path, handle.identity, now)
            .map_err(|error| match error {
                WorkspaceError::LimitReached => "workspace_limit",
                _ => "remote_directory_changed",
            })?;
        Ok(())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn directory_labels_never_become_empty_or_absolute() {
        for path in ["/", "/\n\t", "/   "] {
            assert_eq!(label(Path::new(path)), "Directory");
        }
        assert_eq!(label(Path::new("/parent/project")), "project");
    }

    #[test]
    fn missing_roots_do_not_consume_handles() {
        let state = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let mut projects = RemoteProjects::default();
        projects.authorize(state.path(), root.path()).unwrap();
        std::fs::rename(root.path(), state.path().join("moved-root")).unwrap();
        for _ in 0..1100 {
            assert!(
                projects
                    .list("a", None, None, 100)
                    .unwrap()
                    .entries
                    .is_empty()
            );
        }
        assert!(projects.handles.is_empty());
    }

    #[test]
    fn revisiting_directories_and_pages_reuses_scoped_handles_and_cursors() {
        let state = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        for n in 0..105 {
            std::fs::create_dir(root.path().join(format!("project-{n:03}"))).unwrap();
        }
        let mut projects = RemoteProjects::default();
        projects.authorize(state.path(), root.path()).unwrap();
        let roots = projects.list("a", None, None, 100).unwrap();
        let root_id = &roots.entries[0].directory_id;
        let first = projects.list("a", Some(root_id), None, 100).unwrap();
        let cursor = first.next_cursor.as_deref().unwrap();
        let second = projects
            .list("a", Some(root_id), Some(cursor), 100)
            .unwrap();
        for now in 101..201 {
            assert_eq!(projects.list("a", None, None, now).unwrap(), roots);
            assert_eq!(projects.list("a", Some(root_id), None, now).unwrap(), first);
            assert_eq!(
                projects
                    .list("a", Some(root_id), Some(cursor), now)
                    .unwrap(),
                second
            );
        }
        assert_eq!(projects.handles.len(), 106);
        assert_eq!(projects.cursors.len(), 1);
        assert_ne!(
            projects.list("b", None, None, 200).unwrap().entries[0].directory_id,
            *root_id
        );
    }

    #[test]
    fn handles_are_scoped_expiring_and_never_paths() {
        let state = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("project")).unwrap();
        let mut projects = RemoteProjects::default();
        projects.authorize(state.path(), root.path()).unwrap();
        let page = projects.list("a", None, None, 100).unwrap();
        let root_id = &page.entries[0].directory_id;
        assert!(
            !serde_json::to_string(&page)
                .unwrap()
                .contains(&root.path().display().to_string())
        );
        assert!(projects.list("b", Some(root_id), None, 101).is_err());
        let children = projects.list("a", Some(root_id), None, 101).unwrap();
        let mut index = WorkspaceIndex::new();
        projects
            .enrol("a", &children.entries[0].directory_id, 102, &mut index)
            .unwrap();
        assert_eq!(index.len(), 1);
        assert!(
            projects
                .enrol("b", &children.entries[0].directory_id, 102, &mut index)
                .is_err()
        );
        assert!(projects.list("a", Some(root_id), None, 300_100).is_err());
    }

    #[test]
    fn browser_path_traversal_strings_never_resolve_as_directory_handles() {
        let state = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let mut projects = RemoteProjects::default();
        projects.authorize(state.path(), root.path()).unwrap();
        let roots = projects.list("a", None, None, 100).unwrap();
        let root_id = &roots.entries[0].directory_id;
        let mut index = WorkspaceIndex::new();
        for payload in [
            "..".to_owned(),
            "../outside".to_owned(),
            "/etc".to_owned(),
            "C:\\Windows".to_owned(),
            "%2e%2e%2foutside".to_owned(),
            "project\0/../outside".to_owned(),
            format!("{root_id}/../outside"),
            root.path().to_string_lossy().into_owned(),
        ] {
            assert!(projects.list("a", Some(&payload), None, 101).is_err());
            assert!(
                projects
                    .list("a", Some(root_id), Some(&payload), 101)
                    .is_err()
            );
            assert!(projects.enrol("a", &payload, 101, &mut index).is_err());
            assert_eq!(index.len(), 0);
        }
        projects.enrol("a", root_id, 101, &mut index).unwrap();
        assert_eq!(index.len(), 1);
    }

    #[test]
    fn symlink_escape_replacement_revocation_and_restart_invalidate() {
        use std::os::unix::fs::symlink;
        let state = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        std::fs::create_dir(&project).unwrap();
        symlink(external.path(), root.path().join("escape")).unwrap();
        let mut projects = RemoteProjects::default();
        let grants = projects.authorize(state.path(), root.path()).unwrap();
        let roots = projects.list("a", None, None, 100).unwrap();
        let children = projects
            .list("a", Some(&roots.entries[0].directory_id), None, 101)
            .unwrap();
        assert_eq!(children.entries.len(), 1);
        let child = &children.entries[0].directory_id;
        std::fs::rename(&project, root.path().join("old")).unwrap();
        std::fs::create_dir(&project).unwrap();
        assert!(
            projects
                .enrol("a", child, 102, &mut WorkspaceIndex::new())
                .is_err()
        );
        let restarted = RemoteProjects::load(state.path()).unwrap();
        assert!(
            restarted
                .enrol("a", child, 102, &mut WorkspaceIndex::new())
                .is_err()
        );
        projects.revoke(state.path(), &grants[0].root_id).unwrap();
        assert!(
            projects
                .list("a", Some(&roots.entries[0].directory_id), None, 102)
                .is_err()
        );
        assert!(
            RemoteProjects::load(state.path())
                .unwrap()
                .roots()
                .is_empty()
        );
    }

    #[test]
    fn pagination_is_bounded_scoped_and_complete() {
        let state = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        for n in 0..105 {
            std::fs::create_dir(root.path().join(format!("project-{n:03}"))).unwrap();
        }
        let mut projects = RemoteProjects::default();
        projects.authorize(state.path(), root.path()).unwrap();
        let roots = projects.list("a", None, None, 100).unwrap();
        let id = &roots.entries[0].directory_id;
        let first = projects.list("a", Some(id), None, 100).unwrap();
        assert_eq!(first.entries.len(), 100);
        let cursor = first.next_cursor.unwrap();
        assert!(projects.list("b", Some(id), Some(&cursor), 100).is_err());
        let second = projects.list("a", Some(id), Some(&cursor), 100).unwrap();
        assert_eq!(second.entries.len(), 5);
        assert!(second.next_cursor.is_none());
        assert_eq!(
            projects.list("a", Some(id), Some(&cursor), 100).unwrap(),
            second
        );
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;

    #[test]
    fn reparse_children_are_not_listed_and_replaced_handles_are_refused() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("root");
        let outside = directory.path().join("outside");
        let child = root.join("child");
        std::fs::create_dir_all(&child).unwrap();
        std::fs::create_dir(&outside).unwrap();
        let junction = root.join("escape");
        let created = std::process::Command::new("cmd")
            .args(["/D", "/C", "mklink", "/J"])
            .arg(&junction)
            .arg(&outside)
            .output()
            .unwrap();
        assert!(created.status.success(), "junction creation failed");
        let mut projects = RemoteProjects::default();
        projects.authorize(directory.path(), &root).unwrap();
        let root_id = projects.list("phone", None, None, 1).unwrap().entries[0]
            .directory_id
            .clone();
        let page = projects.list("phone", Some(&root_id), None, 2).unwrap();
        assert_eq!(page.entries.len(), 1);
        assert_eq!(page.entries[0].label, "child");
        let child_id = page.entries[0].directory_id.clone();
        std::fs::rename(&child, root.join("original")).unwrap();
        std::fs::create_dir(&child).unwrap();
        assert_eq!(
            projects
                .list("phone", Some(&child_id), None, 3)
                .unwrap_err(),
            "remote_directory_changed"
        );
        assert!(projects.list("other", Some(&root_id), None, 3).is_err());
        std::fs::remove_dir(junction).unwrap();
    }
}
