use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

const VERSION: &str = "saaio 0.1.0 (abc1234)";

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

fn tarball() -> String {
    format!("saaio-{}.tar.gz", target())
}

fn executable(path: &Path, text: &str) {
    fs::write(path, text).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// A temp folder with the assets of a release: a tarball whose saaio prints `version`, and
/// SHA256SUMS naming it.
struct Release {
    dir: TempDir,
}

impl Release {
    fn new(version: &str) -> Release {
        let dir = TempDir::new().unwrap();
        let build = dir.path().join("build");
        fs::create_dir(&build).unwrap();
        executable(
            &build.join("saaio"),
            &format!("#!/bin/sh\necho '{version}'\n"),
        );
        fs::write(build.join("README.md"), "saaio\n").unwrap();
        let status = Command::new("tar")
            .arg("czf")
            .arg(dir.path().join(tarball()))
            .arg("-C")
            .arg(&build)
            .args(["saaio", "README.md"])
            .status()
            .unwrap();
        assert!(status.success());
        let release = Release { dir };
        release.write_sums(&release.digest());
        release
    }

    fn digest(&self) -> String {
        let output = Command::new("shasum")
            .args(["-a", "256"])
            .arg(self.dir.path().join(tarball()))
            .output()
            .or_else(|_| {
                Command::new("sha256sum")
                    .arg(self.dir.path().join(tarball()))
                    .output()
            })
            .unwrap();
        let text = String::from_utf8(output.stdout).unwrap();
        text.split_whitespace().next().unwrap().to_string()
    }

    fn write_sums(&self, digest: &str) {
        let line = format!("{digest}  {}\n", tarball());
        fs::write(self.dir.path().join("SHA256SUMS"), line).unwrap();
    }

    fn url(&self) -> String {
        format!("file://{}", self.dir.path().display())
    }
}

/// install.sh with only the environment a test gives it.
fn install(home: &Path, path: &str, vars: &[(&str, &str)]) -> Output {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("install.sh");
    Command::new("sh")
        .arg(script)
        .env_clear()
        .env("HOME", home)
        .env("PATH", path)
        .envs(vars.iter().copied())
        .output()
        .unwrap()
}

fn bin(home: &Path) -> PathBuf {
    home.join(".local/bin")
}

fn path_with(dir: &Path) -> String {
    format!("{}:/usr/bin:/bin", dir.display())
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn installs_the_binary_for_this_machine() {
    let home = TempDir::new().unwrap();
    let release = Release::new(VERSION);

    let output = install(
        home.path(),
        &path_with(&bin(home.path())),
        &[("SAAIO_DOWNLOAD_URL", &release.url())],
    );

    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        stdout(&output),
        format!("installed {VERSION} to ~/.local/bin/saaio\n")
    );
    let installed = bin(home.path()).join("saaio");
    let mode = fs::metadata(&installed).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o755);
    let ran = Command::new(&installed).arg("--version").output().unwrap();
    assert_eq!(stdout(&ran), format!("{VERSION}\n"));
}

#[test]
fn replaces_an_installed_binary() {
    let home = TempDir::new().unwrap();
    let dir = home.path().join("tools");
    fs::create_dir(&dir).unwrap();
    executable(
        &dir.join("saaio"),
        "#!/bin/sh\necho 'saaio 0.0.9 (0000000)'\n",
    );
    let release = Release::new(VERSION);

    let output = install(
        home.path(),
        &path_with(&dir),
        &[
            ("SAAIO_DOWNLOAD_URL", &release.url()),
            ("SAAIO_INSTALL_DIR", dir.to_str().unwrap()),
        ],
    );

    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        stdout(&output),
        format!("installed {VERSION} to ~/tools/saaio\n")
    );
    let ran = Command::new(dir.join("saaio")).output().unwrap();
    assert_eq!(stdout(&ran), format!("{VERSION}\n"));
    let left: Vec<_> = fs::read_dir(&dir).unwrap().collect();
    assert_eq!(left.len(), 1, "no temporary file stays beside saaio");
}

#[test]
fn refuses_a_tarball_whose_checksum_differs() {
    let home = TempDir::new().unwrap();
    let release = Release::new(VERSION);
    release.write_sums(&"0".repeat(64));

    let output = install(
        home.path(),
        &path_with(&bin(home.path())),
        &[("SAAIO_DOWNLOAD_URL", &release.url())],
    );

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        stderr(&output),
        format!("saaio: checksum mismatch for {}\n", tarball())
    );
    assert!(!bin(home.path()).join("saaio").exists());
}

#[test]
fn names_the_folder_when_it_is_not_on_path() {
    let home = TempDir::new().unwrap();
    let release = Release::new(VERSION);

    let output = install(
        home.path(),
        "/usr/bin:/bin",
        &[("SAAIO_DOWNLOAD_URL", &release.url())],
    );

    assert!(output.status.success(), "{}", stderr(&output));
    let dir = bin(home.path());
    assert_eq!(
        stdout(&output),
        format!(
            "installed {VERSION} to ~/.local/bin/saaio\n\n\
             ~/.local/bin is not on your PATH. Add this line to your shell's startup file\n\
             (~/.zshrc, ~/.bashrc, ...) and open a new terminal:\n\n  \
             export PATH=\"{}:$PATH\"\n",
            dir.display()
        )
    );
}

#[test]
fn installs_the_version_asked_for() {
    let home = TempDir::new().unwrap();
    let tools = TempDir::new().unwrap();
    let args = tools.path().join("args");
    executable(
        &tools.path().join("curl"),
        &format!("#!/bin/sh\necho \"$@\" >> '{}'\nexit 22\n", args.display()),
    );

    let output = install(
        home.path(),
        &path_with(tools.path()),
        &[("SAAIO_VERSION", "v0.2.0")],
    );

    assert_eq!(output.status.code(), Some(1));
    let url = format!(
        "https://github.com/JanSuthacheeva/saaio/releases/download/v0.2.0/{}",
        tarball()
    );
    assert_eq!(
        stderr(&output),
        format!("saaio: could not download {url}\n")
    );
    let called = fs::read_to_string(&args).unwrap();
    assert!(
        called.starts_with("--proto =https --tlsv1.2 -fsSL ")
            && called.ends_with(&format!(" {url}\n")),
        "{called}"
    );
}
