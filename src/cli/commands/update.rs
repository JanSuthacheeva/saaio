//! `saaio update`: installs the latest release, or a tag, over this binary with its install.sh

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{Context, bail};
use rustix::fs::{Access, access};

use crate::binaries::{self, Binary};
use crate::cli::Env;
use crate::cli::output::update::Others;
use crate::cli::output::{Document, Failure, update as output};
use crate::client;
use crate::release::source::Releases;
use crate::release::{Build, Tag};
use crate::update::{self, Step};

/// How long curl gets to name the latest release or hand over install.sh.
const LIMIT: Duration = Duration::from_secs(15);

#[derive(Debug, clap::Args)]
pub struct Args {
    /// The release to install, such as v0.1.1; default: the latest
    #[arg(long, value_name = "TAG")]
    version: Option<String>,
}

/// Installs `--version` or the latest release into this binary's folder, unless it runs that
/// release already, was built from source or its folder cannot be written. Runs without a store.
pub fn run(args: &Args, env: &Env) -> Result<Document, Failure> {
    let releases = env.releases();
    let target = match &args.version {
        Some(version) => Tag::parse(version).map_err(|error| Failure {
            message: format!("{error:#}"),
            help: vec!["Pass a release tag, such as --version v0.1.1".to_string()],
            code: 2,
        })?,
        None => releases.latest(LIMIT).map_err(|error| {
            Failure::error(
                format!("could not read the latest release: {error:#}"),
                vec!["Check the connection, or pass --version <tag>".to_string()],
            )
        })?,
    };
    let exe = std::env::current_exe()?.canonicalize()?;
    let dir = exe.parent().unwrap_or(Path::new("/"));
    let writable = access(dir, Access::WRITE_OK).is_ok();
    let running = Build::running();
    match update::step(&running, &target, args.version.is_some(), writable) {
        Step::Latest => {
            let others = others(env, &running.version().to_string());
            Ok(output::latest(&running, &target, &env.bin_path(), &others))
        }
        Step::Source => Err(output::source(&target)),
        Step::Unwritable => Err(output::unwritable(
            &dir.display().to_string(),
            &releases,
            &target,
        )),
        Step::Install => {
            install(&releases, &target, dir)
                .map_err(|error| output::failed(&target, &format!("{error:#}"), &releases))?;
            let others = others(env, &target.version().to_string());
            Ok(output::updated(&running, &target, &env.bin_path(), &others))
        }
    }
}

/// The saaio first on PATH when it is not `version`, and the servers of every registered store
/// that are not `version` or refused this one (an earlier protocol); paths with ~. A server that
/// cannot be asked is left out.
fn others(env: &Env, version: &str) -> Others {
    let shadowed = std::env::current_exe().ok().and_then(|exe| {
        let this = Binary {
            exe,
            build: version.to_string(),
        };
        binaries::shadowing(&this, env.path.as_deref()?)
    });
    let stores = env.registry().map(|registry| registry.stores());
    let servers = stores
        .unwrap_or_default()
        .iter()
        .filter_map(|store| {
            let sockets = [Some(env.socket(store)), client::earlier_socket(store)];
            let mut running = sockets
                .into_iter()
                .flatten()
                .find_map(|socket| client::running(&socket).ok().flatten())?;
            if let Some(server) = &mut running.server {
                if server.version() == version {
                    return None;
                }
                server.exe = PathBuf::from(env.tilde(&server.exe));
            }
            Some((PathBuf::from(env.tilde(store.root())), running))
        })
        .collect();
    Others {
        shadowed: shadowed.map(|other| Binary {
            exe: PathBuf::from(env.tilde(&other.exe)),
            build: other.build,
        }),
        servers,
    }
}

/// Runs `tag`'s install.sh with `dir` as its folder: it downloads the tarball, checks it against
/// SHA256SUMS and renames the new binary over the old one. An error with its last line.
fn install(releases: &Releases, tag: &Tag, dir: &Path) -> anyhow::Result<()> {
    let script = releases.fetch(tag, "install.sh", LIMIT)?;
    let mut child = Command::new("sh")
        .arg("-s")
        .env_remove("SAAIO_DOWNLOAD_URL")
        .envs(releases.install_env(tag))
        .env("SAAIO_INSTALL_DIR", dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("could not run sh")?;
    child
        .stdin
        .take()
        .context("no stdin for sh")?
        .write_all(script.as_bytes())?;
    let output = child.wait_with_output()?;
    if !output.status.success() {
        let text = String::from_utf8_lossy(if output.stderr.is_empty() {
            &output.stdout
        } else {
            &output.stderr
        })
        .into_owned();
        let last = text.lines().rfind(|line| !line.trim().is_empty());
        bail!("{}", last.unwrap_or("install.sh failed").trim());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use sha2::{Digest, Sha256};
    use tempfile::TempDir;

    use super::*;
    use crate::fixtures::executable;

    /// The release target install.sh picks on this machine.
    fn target() -> &'static str {
        match (std::env::consts::OS, std::env::consts::ARCH) {
            ("macos", "aarch64") => "aarch64-apple-darwin",
            ("macos", "x86_64") => "x86_64-apple-darwin",
            ("linux", "x86_64") => "x86_64-unknown-linux-musl",
            ("linux", "aarch64") => "aarch64-unknown-linux-musl",
            (os, arch) => panic!("no release target for {os} {arch}"),
        }
    }

    /// A releases page in `dir` with release `tag`: this repo's install.sh, a tarball whose saaio
    /// prints `saaio <version> (abc1234)`, and SHA256SUMS naming `digest`, else the tarball's.
    fn mirror(dir: &Path, tag: &str, digest: Option<&str>) -> Releases {
        let assets = dir.join("download").join(tag);
        fs::create_dir_all(&assets).unwrap();
        let install = Path::new(env!("CARGO_MANIFEST_DIR")).join("install.sh");
        fs::copy(install, assets.join("install.sh")).unwrap();
        let build = dir.join("build");
        fs::create_dir_all(&build).unwrap();
        let version = tag.trim_start_matches('v');
        executable(
            &build.join("saaio"),
            &format!("#!/bin/sh\necho 'saaio {version} (abc1234)'\n"),
        );
        let tarball = format!("saaio-{}.tar.gz", target());
        let status = Command::new("tar")
            .arg("czf")
            .arg(assets.join(&tarball))
            .arg("-C")
            .arg(&build)
            .arg("saaio")
            .status()
            .unwrap();
        assert!(status.success());
        let actual = format!(
            "{:x}",
            Sha256::digest(fs::read(assets.join(&tarball)).unwrap())
        );
        let digest = digest.unwrap_or(&actual);
        fs::write(assets.join("SHA256SUMS"), format!("{digest}  {tarball}\n")).unwrap();
        Releases::at(Some(format!("file://{}", dir.display())))
    }

    fn installed(dir: &Path) -> String {
        let output = Command::new(dir.join("saaio")).output().unwrap();
        String::from_utf8(output.stdout).unwrap()
    }

    #[test]
    fn installs_a_release_over_the_binary_in_its_folder() {
        let dir = TempDir::new().unwrap();
        let releases = mirror(dir.path(), "v0.1.1", None);
        let bin: PathBuf = dir.path().join("bin");
        fs::create_dir(&bin).unwrap();
        executable(
            &bin.join("saaio"),
            "#!/bin/sh\necho 'saaio 0.1.0 (0000000)'\n",
        );

        install(&releases, &Tag::parse("v0.1.1").unwrap(), &bin).unwrap();

        assert_eq!(installed(&bin), "saaio 0.1.1 (abc1234)\n");
    }

    #[test]
    fn names_another_version_first_on_path_with_tilde() {
        let home = TempDir::new().unwrap();
        let cargo = home.path().join(".cargo/bin");
        fs::create_dir_all(&cargo).unwrap();
        executable(
            &cargo.join("saaio"),
            "#!/bin/sh\necho 'saaio 0.1.0 (9b77d8c)'\n",
        );
        // its first exec can take past the probe's 1 s on macOS
        assert!(
            Command::new(cargo.join("saaio"))
                .status()
                .unwrap()
                .success()
        );
        let env = Env {
            home: Some(home.path().to_path_buf()),
            path: Some(cargo.clone().into_os_string()),
            ..Env::default()
        };

        assert_eq!(
            others(&env, "0.2.0"),
            Others {
                shadowed: Some(Binary {
                    exe: "~/.cargo/bin/saaio".into(),
                    build: "0.1.0 (9b77d8c)".into(),
                }),
                servers: Vec::new(),
            }
        );
        assert_eq!(others(&env, "0.1.0"), Others::default());
    }

    #[test]
    fn a_tarball_that_fails_its_checksum_is_not_installed() {
        let dir = TempDir::new().unwrap();
        let releases = mirror(dir.path(), "v0.1.1", Some(&"0".repeat(64)));
        let bin = dir.path().join("bin");
        fs::create_dir(&bin).unwrap();
        executable(
            &bin.join("saaio"),
            "#!/bin/sh\necho 'saaio 0.1.0 (0000000)'\n",
        );

        let error = install(&releases, &Tag::parse("v0.1.1").unwrap(), &bin).unwrap_err();

        assert_eq!(
            format!("{error:#}"),
            format!("saaio: checksum mismatch for saaio-{}.tar.gz", target())
        );
        assert_eq!(installed(&bin), "saaio 0.1.0 (0000000)\n");
    }
}
