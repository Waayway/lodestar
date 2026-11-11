//! Landlock confinement of every `nix` the server runs on a channel's flake. The server's user
//! can read the deploy key, the builder key, the OIDC client secret and the state directory;
//! Nix runs as that user, and Nix reads whatever local source a flake names (an unlocked
//! `path:` input is read, and copied to the world-readable store, before Nix refuses to lock
//! it). Confined, the Nix process (and everything it starts) can read only what an evaluation
//! needs, enforced by the kernel and kept across `exec`.
//!
//! Readable: the Nix store, Nix's configuration, TLS roots and resolver files, the worktree
//! (and its git directory), and a private home, cache and temporary directory under
//! `<state_dir>/nix-sandbox`. Commands that talk to the remote builder also get its SSH key
//! and `~/.ssh` (for `known_hosts`). Nothing else under the state directory, and no other
//! secret, is readable. Without Landlock (Linux 5.13+, enabled in the kernel's `lsm=`) no Nix
//! command runs at all.

use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};

use anyhow::Context;
use tokio::process::Command;

/// What a confined `nix` may do besides evaluating.
#[derive(Clone, Copy, Debug)]
pub enum Access<'a> {
    /// Evaluate or build locally: the worktree is read-only.
    Evaluate,
    /// `nix flake update`: the worktree is writable (for `flake.lock`).
    Update,
    /// Talk to the remote builder over SSH: its key and `~/.ssh` too.
    Builder { key: Option<&'a Path> },
}

/// The private directories of confined `nix` runs, under `dir`.
#[derive(Clone, Debug)]
pub struct NixSandbox {
    dir: PathBuf,
    /// Further readable and writable paths (tests: where their fake `nix` lives).
    extra: Vec<PathBuf>,
}

/// Directories Nix (and git, ssh) read: the store and system configuration.
const READ_DIRECTORIES: &[&str] = &[
    "/nix/store",
    "/nix/var/nix",
    "/etc/nix",
    "/etc/static",
    "/etc/ssl",
    "/etc/pki",
    "/etc/ssh",
    "/run/current-system",
];

/// Single files Nix (curl, git, ssh) read.
const READ_FILES: &[&str] = &[
    "/etc/resolv.conf",
    "/etc/hosts",
    "/etc/nsswitch.conf",
    "/etc/passwd",
    "/etc/group",
    "/etc/services",
    "/etc/protocols",
    "/etc/localtime",
];

/// Devices Nix reads and writes.
const DEVICES: &[&str] = &["/dev/null", "/dev/zero", "/dev/random", "/dev/urandom"];

impl NixSandbox {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            extra: Vec::new(),
        }
    }

    /// Also lets confined commands read and write beneath `path`.
    pub fn also_allow(mut self, path: impl Into<PathBuf>) -> Self {
        self.extra.push(path.into());
        self
    }

    /// Confines `command` (to run in `worktree`) for `access`: points `HOME`, the XDG
    /// directories and `TMPDIR` at the sandbox, drops `SSH_AUTH_SOCK`, and restricts the child
    /// before it `exec`s. The returned ruleset must live until the command has been spawned.
    pub fn confine(
        &self,
        command: &mut Command,
        worktree: &Path,
        access: Access<'_>,
    ) -> anyhow::Result<OwnedFd> {
        let home = self.private_dir("home")?;
        let cache = self.private_dir("cache")?;
        let tmp = self.private_dir("tmp")?;
        let mut rules: Vec<(PathBuf, bool)> = Vec::new();
        rules.extend(READ_DIRECTORIES.iter().map(|path| (path.into(), false)));
        rules.extend(READ_FILES.iter().map(|path| (path.into(), false)));
        rules.extend(DEVICES.iter().map(|path| (path.into(), true)));
        rules.extend([
            (home.clone(), true),
            (cache.clone(), true),
            (tmp.clone(), true),
        ]);
        rules.push((worktree.to_path_buf(), matches!(access, Access::Update)));
        rules.extend(self.extra.iter().map(|path| (path.clone(), true)));
        // A linked worktree's git directory (and the repository's common directory) live
        // outside it.
        rules.extend(git_dirs(worktree).into_iter().map(|path| (path, false)));
        if let Access::Builder { key } = access {
            if let Some(key) = key {
                rules.push((key.to_path_buf(), false));
            }
            // ssh keeps `known_hosts` in the passwd home (the server's `HOME`), not in the
            // sandbox's; only that file (and `config`) is reachable, never a key next to it.
            if let Some(ssh) = std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".ssh"))
            {
                let known_hosts = ssh.join("known_hosts");
                if !known_hosts.exists() {
                    create_private(&ssh)?;
                    std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(&known_hosts)
                        .with_context(|| format!("creating {}", known_hosts.display()))?;
                }
                rules.push((known_hosts, true));
                rules.push((ssh.join("config"), false));
            }
        }
        let ruleset = landlock::ruleset(&rules)?;
        command
            .env("HOME", &home)
            .env("XDG_CACHE_HOME", &cache)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("TMPDIR", &tmp)
            .env_remove("SSH_AUTH_SOCK");
        landlock::restrict_on_exec(command, &ruleset);
        Ok(ruleset)
    }

    fn private_dir(&self, name: &str) -> anyhow::Result<PathBuf> {
        let path = self.dir.join(name);
        create_private(&path)?;
        Ok(path)
    }
}

/// Creates `path` (and its parents) and makes it `0700`.
fn create_private(path: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(path).with_context(|| format!("creating {}", path.display()))?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
        .with_context(|| format!("restricting {}", path.display()))
}

/// The git directory a linked worktree's `.git` file names, and its `commondir`.
fn git_dirs(worktree: &Path) -> Vec<PathBuf> {
    let Ok(text) = std::fs::read_to_string(worktree.join(".git")) else {
        return Vec::new();
    };
    let Some(git_dir) = text.trim().strip_prefix("gitdir: ") else {
        return Vec::new();
    };
    let git_dir = worktree.join(git_dir);
    let mut dirs = vec![git_dir.clone()];
    if let Ok(common) = std::fs::read_to_string(git_dir.join("commondir")) {
        dirs.push(git_dir.join(common.trim()));
    }
    dirs
}

#[cfg(target_os = "linux")]
mod landlock {
    //! The three Landlock system calls, through `libc`.

    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::os::unix::ffi::OsStrExt;
    use std::path::PathBuf;

    use anyhow::{Context, bail};
    use tokio::process::Command;

    const CREATE_RULESET_VERSION: u32 = 1;
    const RULE_PATH_BENEATH: libc::c_int = 1;

    const EXECUTE: u64 = 1 << 0;
    const WRITE_FILE: u64 = 1 << 1;
    const READ_FILE: u64 = 1 << 2;
    const READ_DIR: u64 = 1 << 3;
    const REMOVE_DIR: u64 = 1 << 4;
    const REMOVE_FILE: u64 = 1 << 5;
    const MAKE_CHAR: u64 = 1 << 6;
    const MAKE_DIR: u64 = 1 << 7;
    const MAKE_REG: u64 = 1 << 8;
    const MAKE_SOCK: u64 = 1 << 9;
    const MAKE_FIFO: u64 = 1 << 10;
    const MAKE_BLOCK: u64 = 1 << 11;
    const MAKE_SYM: u64 = 1 << 12;
    /// ABI 2.
    const REFER: u64 = 1 << 13;
    /// ABI 3.
    const TRUNCATE: u64 = 1 << 14;

    const READ: u64 = EXECUTE | READ_FILE | READ_DIR;
    const WRITE: u64 = WRITE_FILE
        | REMOVE_DIR
        | REMOVE_FILE
        | MAKE_DIR
        | MAKE_REG
        | MAKE_SOCK
        | MAKE_FIFO
        | MAKE_SYM
        | REFER
        | TRUNCATE;
    /// The rights a rule on a file (not a directory) may carry.
    const FILE_RIGHTS: u64 = EXECUTE | WRITE_FILE | READ_FILE | TRUNCATE;

    #[repr(C)]
    struct RulesetAttr {
        handled_access_fs: u64,
    }

    #[repr(C, packed)]
    struct PathBeneathAttr {
        allowed_access: u64,
        parent_fd: i32,
    }

    /// The filesystem rights the running kernel's Landlock handles.
    fn handled() -> anyhow::Result<u64> {
        // SAFETY: the documented way to ask for the ABI version: no attribute, no size.
        let abi = unsafe {
            libc::syscall(
                libc::SYS_landlock_create_ruleset,
                std::ptr::null::<RulesetAttr>(),
                0usize,
                CREATE_RULESET_VERSION,
            )
        };
        if abi < 1 {
            bail!(
                "Landlock is not available ({}): refusing to run nix on a channel's flake",
                std::io::Error::last_os_error()
            );
        }
        let mut rights = READ
            | WRITE_FILE
            | REMOVE_DIR
            | REMOVE_FILE
            | MAKE_CHAR
            | MAKE_DIR
            | MAKE_REG
            | MAKE_SOCK
            | MAKE_FIFO
            | MAKE_BLOCK
            | MAKE_SYM;
        if abi >= 2 {
            rights |= REFER;
        }
        if abi >= 3 {
            rights |= TRUNCATE;
        }
        Ok(rights)
    }

    /// A ruleset that allows reading (and, where the flag is set, writing) beneath each path
    /// and nothing else. Paths that do not exist are left out.
    pub fn ruleset(rules: &[(PathBuf, bool)]) -> anyhow::Result<OwnedFd> {
        let handled = handled()?;
        let attr = RulesetAttr {
            handled_access_fs: handled,
        };
        // SAFETY: `attr` is a valid `landlock_ruleset_attr` prefix of the given size.
        let fd = unsafe {
            libc::syscall(
                libc::SYS_landlock_create_ruleset,
                &attr as *const RulesetAttr,
                std::mem::size_of::<RulesetAttr>(),
                0u32,
            )
        };
        if fd < 0 {
            return Err(std::io::Error::last_os_error()).context("creating a Landlock ruleset");
        }
        // SAFETY: the system call returned a new file descriptor that nothing else owns.
        let ruleset = unsafe { OwnedFd::from_raw_fd(fd as i32) };
        for (path, writable) in rules {
            let Ok(c_path) = CString::new(path.as_os_str().as_bytes()) else {
                continue;
            };
            // SAFETY: a NUL-terminated path; the descriptor is closed below.
            let parent = unsafe { libc::open(c_path.as_ptr(), libc::O_PATH | libc::O_CLOEXEC) };
            if parent < 0 {
                continue;
            }
            // SAFETY: `parent` is the descriptor just opened.
            let parent = unsafe { OwnedFd::from_raw_fd(parent) };
            let mut allowed = if *writable { READ | WRITE } else { READ };
            if !path.is_dir() {
                allowed &= FILE_RIGHTS;
            }
            let rule = PathBeneathAttr {
                allowed_access: allowed & handled,
                parent_fd: parent.as_raw_fd(),
            };
            // SAFETY: valid ruleset descriptor and `landlock_path_beneath_attr`.
            let added = unsafe {
                libc::syscall(
                    libc::SYS_landlock_add_rule,
                    ruleset.as_raw_fd(),
                    RULE_PATH_BENEATH,
                    &rule as *const PathBeneathAttr,
                    0u32,
                )
            };
            if added < 0 {
                return Err(std::io::Error::last_os_error()).with_context(|| {
                    format!("allowing {} in the Landlock ruleset", path.display())
                });
            }
        }
        Ok(ruleset)
    }

    /// Restricts the child of `command` with `ruleset` between `fork` and `exec`; when that
    /// fails, the command fails to spawn.
    pub fn restrict_on_exec(command: &mut Command, ruleset: &OwnedFd) {
        let fd = ruleset.as_raw_fd();
        // SAFETY: only async-signal-safe system calls run in the forked child.
        unsafe {
            command.pre_exec(move || {
                if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::syscall(libc::SYS_landlock_restrict_self, fd, 0u32) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod landlock {
    use std::os::fd::OwnedFd;
    use std::path::PathBuf;

    use tokio::process::Command;

    pub fn ruleset(_rules: &[(PathBuf, bool)]) -> anyhow::Result<OwnedFd> {
        anyhow::bail!("Landlock is Linux-only: refusing to run nix on a channel's flake")
    }

    pub fn restrict_on_exec(_command: &mut Command, _ruleset: &OwnedFd) {}
}
