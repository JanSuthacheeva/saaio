use std::ffi::OsStr;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::store::Store;

/// The folder every store registers in and slots are claimed in: the default store's folder,
/// $XDG_STATE_HOME/flow or ~/.local/state/flow, whatever FLOW_HOME says.
#[derive(Debug, Clone)]
pub struct Registry {
    root: PathBuf, // holds stores/<hash> and slots/<n>
}

impl Registry {
    /// None when neither directory is set.
    pub fn locate(xdg_state: Option<PathBuf>, home: Option<PathBuf>) -> Option<Registry> {
        Registry::default_root(xdg_state, home).map(|root| Registry { root })
    }

    /// $XDG_STATE_HOME/flow, else $HOME/.local/state/flow; an empty value counts as unset.
    pub fn default_root(xdg_state: Option<PathBuf>, home: Option<PathBuf>) -> Option<PathBuf> {
        let set = |path: Option<PathBuf>| path.filter(|path| !path.as_os_str().is_empty());
        set(xdg_state)
            .map(|state| state.join("flow"))
            .or_else(|| set(home).map(|home| home.join(".local/state/flow")))
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
        let fresh = entry.with_extension(format!("{}.tmp", std::process::id()));
        let mut line = store.root().as_os_str().as_bytes().to_vec();
        line.push(b'\n');
        fs::write(&fresh, line)
            .and_then(|()| fs::rename(&fresh, &entry))
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

    fn entry(&self, root: &Path) -> PathBuf {
        self.root.join("stores").join(format!(
            "{:016x}",
            fnv1a(root.as_os_str().as_encoded_bytes())
        ))
    }
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
        Registry::at(dir.path().join("flow"))
    }

    fn store(dir: &TempDir, name: &str) -> Store {
        let root = dir.path().join("flow/projects").join(name);
        fs::create_dir_all(&root).unwrap();
        Store::at(root)
    }

    #[test]
    fn locates_the_shared_folder_without_flow_home() {
        let registry = Registry::locate(Some("/state".into()), Some("/home".into())).unwrap();
        assert_eq!(registry.root(), Path::new("/state/flow"));

        let registry = Registry::locate(None, Some("/home".into())).unwrap();
        assert_eq!(registry.root(), Path::new("/home/.local/state/flow"));

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
}
