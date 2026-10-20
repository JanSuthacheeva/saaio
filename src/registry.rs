use std::ffi::OsStr;
use std::fs;
use std::io::{self, ErrorKind};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::store::{Store, write_atomic};

/// The file in a project store that names the git dir of the repository owning it.
const OWNER_FILE: &str = "repo";
/// The folder under the registry's root holding one store per repository.
const PROJECTS_DIR: &str = "projects";

/// Who owns a candidate project store, read from its repo file.
#[derive(Debug)]
enum Owner {
    /// No repo file.
    Free,
    /// The git dir the file names; empty when it names nothing or cannot be read, so it is taken
    /// over like a repository that is gone.
    Repo(PathBuf),
}

/// The folder every store registers in and slots are claimed in: the default store's folder,
/// $XDG_STATE_HOME/saaio or ~/.local/state/saaio, whatever SAAIO_HOME says.
#[derive(Debug, Clone)]
pub struct Registry {
    root: PathBuf, // holds stores/<hash> and slots/<n>
}

impl Registry {
    /// None when neither directory is set.
    pub fn locate(xdg_state: Option<PathBuf>, home: Option<PathBuf>) -> Option<Registry> {
        Registry::default_root(xdg_state, home).map(|root| Registry { root })
    }

    /// $XDG_STATE_HOME/saaio, else $HOME/.local/state/saaio; an empty value counts as unset.
    fn default_root(xdg_state: Option<PathBuf>, home: Option<PathBuf>) -> Option<PathBuf> {
        let set = |path: Option<PathBuf>| path.filter(|path| !path.as_os_str().is_empty());
        set(xdg_state)
            .map(|state| state.join("saaio"))
            .or_else(|| set(home).map(|home| home.join(".local/state/saaio")))
    }

    #[cfg(test)]
    pub fn at(root: PathBuf) -> Registry {
        Registry { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Writes stores/<hash of the store's root> when it is missing; cheap enough for every command.
    pub fn register(&self, store: &Store) -> Result<()> {
        let entry = self.entry(store.root());
        if entry.exists() {
            return Ok(());
        }
        let stores = self.root.join("stores");
        fs::create_dir_all(&stores)
            .with_context(|| format!("could not create {}", stores.display()))?;
        let mut line = store.root().as_os_str().as_bytes().to_vec();
        line.push(b'\n');
        write_atomic(&entry, &line)
            .with_context(|| format!("could not register {}", store.root().display()))
    }

    /// Every registered store whose folder still exists, sorted by project name; a vanished one is
    /// dropped from the registry.
    pub fn stores(&self) -> Vec<Store> {
        let Ok(entries) = fs::read_dir(self.root.join("stores")) else {
            return Vec::new();
        };
        let mut stores: Vec<Store> = entries
            .flatten()
            .filter(|entry| !entry.file_name().to_string_lossy().contains('.'))
            .filter_map(|entry| {
                let line = fs::read(entry.path()).ok()?;
                let root = PathBuf::from(OsStr::from_bytes(line.strip_suffix(b"\n")?));
                if root.is_dir() {
                    Some(Store::at(root))
                } else {
                    let _ = fs::remove_file(entry.path());
                    None
                }
            })
            .collect();
        stores.sort_by_cached_key(|store| (project_name(store, self), store.root().to_path_buf()));
        stores
    }

    /// The store of the repository whose git dir is `git_dir`: projects/<name>, or <name>-<n> when
    /// another repository that still exists owns that name. Writes the repo file when the store
    /// has none or names a repository that is gone.
    pub fn project_store(&self, git_dir: &Path) -> Result<Store> {
        let base = repo_name(git_dir)
            .with_context(|| format!("no project name in {}", git_dir.display()))?;
        let mut number = 1;
        loop {
            let name = match number {
                1 => base.clone(),
                _ => format!("{base}-{number}"),
            };
            let root = self.root.join(PROJECTS_DIR).join(name);
            let claimed = match owner(&root) {
                Owner::Repo(repo) if repo == git_dir => return Ok(Store::at(root)),
                Owner::Repo(repo) if repo.exists() => {
                    number += 1;
                    continue;
                }
                Owner::Repo(_) => claim(&root, git_dir, false),
                Owner::Free => claim(&root, git_dir, true),
            };
            match claimed {
                Ok(()) => return Ok(Store::at(root)),
                // Another process claimed it meanwhile: look at the same name again.
                Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("could not claim {}", root.display()));
                }
            }
        }
    }

    fn entry(&self, root: &Path) -> PathBuf {
        self.root.join("stores").join(format!(
            "{:016x}",
            fnv1a(root.as_os_str().as_encoded_bytes())
        ))
    }
}

/// The project name of a git dir: the repository's folder for <repo>/.git, else the dir's own
/// name without .git; None for a root path.
fn repo_name(git_dir: &Path) -> Option<String> {
    let name = git_dir.file_name()?.to_string_lossy();
    if name == ".git" {
        return git_dir
            .parent()?
            .file_name()
            .map(|name| name.to_string_lossy().into_owned());
    }
    Some(name.strip_suffix(".git").unwrap_or(&name).to_string())
}

fn owner(store: &Path) -> Owner {
    match fs::read(store.join(OWNER_FILE)) {
        Err(error) if error.kind() == ErrorKind::NotFound => Owner::Free,
        Err(_) => Owner::Repo(PathBuf::new()),
        Ok(line) => {
            let path = line.strip_suffix(b"\n").unwrap_or(&line);
            Owner::Repo(PathBuf::from(OsStr::from_bytes(path)))
        }
    }
}

/// Writes the repo file: linked into place whole for a free store (Err AlreadyExists when another
/// process won the race), write_atomic over a gone owner.
fn claim(store: &Path, git_dir: &Path, free: bool) -> io::Result<()> {
    fs::create_dir_all(store)?;
    let mut line = git_dir.as_os_str().as_bytes().to_vec();
    line.push(b'\n');
    let file = store.join(OWNER_FILE);
    if !free {
        return write_atomic(&file, &line);
    }
    let partial = store.join(format!(".{OWNER_FILE}.{}", std::process::id()));
    fs::write(&partial, &line)?;
    let linked = fs::hard_link(&partial, &file);
    let _ = fs::remove_file(&partial);
    linked
}

/// A store's project name: its folder's name (".../projects/shop" -> "shop"), "default" for the
/// registry's own folder.
pub fn project_name(store: &Store, registry: &Registry) -> String {
    if same_folder(store.root(), registry.root()) {
        return "default".to_string();
    }
    store
        .root()
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| store.root().display().to_string())
}

fn same_folder(left: &Path, right: &Path) -> bool {
    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

/// A hash that stays the same across builds, so every process finds the same file.
pub fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    fn registry(dir: &TempDir) -> Registry {
        Registry::at(dir.path().join("saaio"))
    }

    fn store(dir: &TempDir, name: &str) -> Store {
        let root = dir.path().join("saaio/projects").join(name);
        fs::create_dir_all(&root).unwrap();
        Store::at(root)
    }

    #[test]
    fn locates_the_shared_folder_without_flow_home() {
        let registry = Registry::locate(Some("/state".into()), Some("/home".into())).unwrap();
        assert_eq!(registry.root(), Path::new("/state/saaio"));

        let registry = Registry::locate(None, Some("/home".into())).unwrap();
        assert_eq!(registry.root(), Path::new("/home/.local/state/saaio"));

        assert!(Registry::locate(Some("".into()), None).is_none());
    }

    #[test]
    fn registers_a_store_once() {
        let dir = TempDir::new().unwrap();
        let registry = registry(&dir);
        let shop = store(&dir, "shop");

        registry.register(&shop).unwrap();
        registry.register(&shop).unwrap();

        assert_eq!(
            fs::read_dir(registry.root().join("stores"))
                .unwrap()
                .count(),
            1
        );
        let roots: Vec<PathBuf> = registry
            .stores()
            .iter()
            .map(|store| store.root().to_path_buf())
            .collect();
        assert_eq!(roots, vec![shop.root().to_path_buf()]);
    }

    #[test]
    fn lists_stores_by_project_name_and_drops_vanished_ones() {
        let dir = TempDir::new().unwrap();
        let registry = registry(&dir);
        let web = store(&dir, "web");
        let api = store(&dir, "api");
        let gone = store(&dir, "gone");
        for store in [&web, &api, &gone] {
            registry.register(store).unwrap();
        }
        fs::remove_dir_all(gone.root()).unwrap();

        let names: Vec<String> = registry
            .stores()
            .iter()
            .map(|store| project_name(store, &registry))
            .collect();

        assert_eq!(names, vec!["api", "web"]);
        assert_eq!(
            fs::read_dir(registry.root().join("stores"))
                .unwrap()
                .count(),
            2
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn registers_a_store_whose_path_is_not_utf8() {
        let dir = TempDir::new().unwrap();
        let registry = registry(&dir);
        let root = dir.path().join(OsStr::from_bytes(b"caf\xe9"));
        fs::create_dir_all(&root).unwrap();

        registry.register(&Store::at(root.clone())).unwrap();

        let roots: Vec<PathBuf> = registry
            .stores()
            .iter()
            .map(|store| store.root().to_path_buf())
            .collect();
        assert_eq!(roots, vec![root]);
    }

    #[test]
    fn names_the_shared_folder_default() {
        let dir = TempDir::new().unwrap();
        let registry = registry(&dir);
        fs::create_dir_all(registry.root()).unwrap();

        assert_eq!(
            project_name(&Store::at(registry.root().to_path_buf()), &registry),
            "default"
        );
        assert_eq!(project_name(&store(&dir, "shop"), &registry), "shop");
    }

    fn git_dir(dir: &TempDir, repo: &str) -> PathBuf {
        let git_dir = dir.path().join(repo).join(".git");
        fs::create_dir_all(&git_dir).unwrap();
        git_dir
    }

    fn project_store(registry: &Registry, git_dir: &Path) -> PathBuf {
        registry
            .project_store(git_dir)
            .unwrap()
            .root()
            .to_path_buf()
    }

    #[test]
    fn names_a_repository_by_its_folder_or_its_git_dir() {
        assert_eq!(
            repo_name(Path::new("/code/shop/.git")).as_deref(),
            Some("shop")
        );
        assert_eq!(
            repo_name(Path::new("/srv/shop.git")).as_deref(),
            Some("shop")
        );
        assert_eq!(
            repo_name(Path::new("/code/app/.git/modules/lib")).as_deref(),
            Some("lib")
        );
        assert_eq!(repo_name(Path::new("/")), None);
    }

    #[test]
    fn claims_a_project_store_and_finds_it_again() {
        let dir = TempDir::new().unwrap();
        let registry = registry(&dir);
        let shop = git_dir(&dir, "code/shop");

        let root = project_store(&registry, &shop);

        assert_eq!(root, registry.root().join("projects/shop"));
        assert_eq!(
            fs::read(root.join("repo")).unwrap(),
            format!("{}\n", shop.display()).into_bytes()
        );
        assert_eq!(project_store(&registry, &shop), root);
    }

    #[test]
    fn numbers_a_second_repository_of_the_same_name() {
        let dir = TempDir::new().unwrap();
        let registry = registry(&dir);
        let first = git_dir(&dir, "code/shop");
        let second = git_dir(&dir, "work/shop");
        let third = git_dir(&dir, "old/shop");

        project_store(&registry, &first);

        assert_eq!(
            project_store(&registry, &second),
            registry.root().join("projects/shop-2")
        );
        assert_eq!(
            project_store(&registry, &third),
            registry.root().join("projects/shop-3")
        );
        assert_eq!(
            project_store(&registry, &first),
            registry.root().join("projects/shop")
        );
    }

    #[test]
    fn adopts_a_store_that_names_no_repository() {
        let dir = TempDir::new().unwrap();
        let registry = registry(&dir);
        let existing = store(&dir, "shop");
        fs::create_dir_all(existing.root().join("flows")).unwrap();
        let shop = git_dir(&dir, "code/shop");

        assert_eq!(project_store(&registry, &shop), existing.root());
        assert!(existing.root().join("flows").is_dir());
        assert!(existing.root().join("repo").is_file());
    }

    #[test]
    fn takes_over_a_store_whose_repo_file_names_nothing() {
        let dir = TempDir::new().unwrap();
        let registry = registry(&dir);
        let existing = store(&dir, "shop");
        fs::write(existing.root().join("repo"), "").unwrap();
        let shop = git_dir(&dir, "code/shop");

        assert_eq!(project_store(&registry, &shop), existing.root());
        assert_eq!(
            fs::read(existing.root().join("repo")).unwrap(),
            format!("{}\n", shop.display()).into_bytes()
        );
    }

    #[test]
    fn reads_a_repo_file_without_its_newline() {
        let dir = TempDir::new().unwrap();
        let registry = registry(&dir);
        let first = git_dir(&dir, "code/shop");
        let existing = store(&dir, "shop");
        fs::write(existing.root().join("repo"), first.as_os_str().as_bytes()).unwrap();
        let second = git_dir(&dir, "work/shop");

        assert_eq!(project_store(&registry, &first), existing.root());
        assert_eq!(
            project_store(&registry, &second),
            registry.root().join("projects/shop-2")
        );
    }

    #[test]
    fn takes_over_a_store_whose_repository_is_gone() {
        let dir = TempDir::new().unwrap();
        let registry = registry(&dir);
        let moved = git_dir(&dir, "old/shop");
        project_store(&registry, &moved);
        fs::remove_dir_all(dir.path().join("old")).unwrap();
        let shop = git_dir(&dir, "code/shop");

        assert_eq!(
            project_store(&registry, &shop),
            registry.root().join("projects/shop")
        );
        assert_eq!(
            project_store(&registry, &shop),
            registry.root().join("projects/shop")
        );
    }
}
