//! `Context`: what every command gets, and the store a command opens when it needs one

use super::Env;
use super::output::Failure;
use crate::registry::Registry;
use crate::store::Store;

/// The environment and the clock, handed to every command.
pub struct Context<'a> {
    pub env: &'a Env,
    pub clock: fn() -> u64,
}

/// The store a command works on and the shared folder it is registered in.
pub struct Opened {
    pub store: Store,
    pub registry: Registry,
}

impl Context<'_> {
    /// The clock read once.
    pub fn now(&self) -> u64 {
        (self.clock)()
    }

    /// The store Env::store picks for the current folder and the registry, the store registered
    /// there (best effort: only the overview and the slots need it).
    pub fn open(&self) -> Result<Opened, Failure> {
        let cwd = std::env::current_dir().ok();
        let store = self.env.store(cwd.as_deref()).map_err(|error| {
            Failure::error(
                format!("could not open the store: {error:#}"),
                vec!["Set SAAIO_HOME to a directory".to_string()],
            )
        })?;
        let Some(store) = store else {
            return Err(Failure::error(
                "cannot find a state directory",
                vec!["Set SAAIO_HOME to a directory".to_string()],
            ));
        };
        let Some(registry) = self.env.registry() else {
            return Err(Failure::error(
                "cannot find a state directory",
                vec!["Set HOME or XDG_STATE_HOME to a directory".to_string()],
            ));
        };
        let _ = registry.register(&store);
        Ok(Opened { store, registry })
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn opens_the_store_and_registers_it() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir(dir.path().join("store")).unwrap();
        let env = Env {
            saaio_home: Some(dir.path().join("store")),
            xdg_state_home: Some(dir.path().join("state")),
            ..Env::default()
        };
        let ctx = Context {
            env: &env,
            clock: || 7,
        };

        let opened = ctx.open().unwrap();

        assert_eq!(ctx.now(), 7);
        assert_eq!(opened.store.root(), dir.path().join("store"));
        let registered: Vec<_> = opened
            .registry
            .stores()
            .iter()
            .map(|store| store.root().to_path_buf())
            .collect();
        assert_eq!(registered, vec![dir.path().join("store")]);
    }

    #[test]
    fn fails_without_a_state_directory() {
        let env = Env::default();
        let ctx = Context {
            env: &env,
            clock: || 0,
        };

        let failure = ctx.open().err().unwrap();

        assert_eq!(failure.message, "cannot find a state directory");
        assert_eq!(
            failure.help,
            vec!["Set SAAIO_HOME to a directory".to_string()]
        );
    }

    #[test]
    fn fails_without_a_shared_folder() {
        let dir = TempDir::new().unwrap();
        let env = Env {
            saaio_home: Some(dir.path().to_path_buf()),
            ..Env::default()
        };
        let ctx = Context {
            env: &env,
            clock: || 0,
        };

        let failure = ctx.open().err().unwrap();

        assert_eq!(
            failure.help,
            vec!["Set HOME or XDG_STATE_HOME to a directory".to_string()]
        );
    }
}
