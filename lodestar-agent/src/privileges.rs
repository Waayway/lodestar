//! Dropping root privileges for commands that run as the configured user.

use std::os::unix::process::CommandExt;
use std::process::Command;

use nix::unistd::{Gid, Uid, setgid, setuid};

/// Element type `getgrouplist` fills: `c_int` on Apple, `gid_t` elsewhere.
#[cfg(target_vendor = "apple")]
type GroupElem = libc::c_int;
#[cfg(not(target_vendor = "apple"))]
type GroupElem = libc::gid_t;

/// The user's group list (including `gid`), resolved in the parent process.
/// `getgrouplist` may allocate and consult NSS, so it must not run after fork.
fn group_list(user: &std::ffi::CStr, gid: u32) -> std::io::Result<Vec<libc::gid_t>> {
    let mut buffer: Vec<GroupElem> = vec![0; 32];
    loop {
        let mut count = buffer.len() as libc::c_int;
        // SAFETY: `user` is NUL-terminated; `buffer` holds `count` elements and `count` is
        // a valid out-pointer for the duration of the call.
        let result =
            unsafe { libc::getgrouplist(user.as_ptr(), gid as _, buffer.as_mut_ptr(), &mut count) };
        if result >= 0 {
            buffer.truncate(count as usize);
            return Ok(buffer
                .into_iter()
                .map(|group| group as libc::gid_t)
                .collect());
        }
        // -1: buffer too small, `count` now holds the required size. Grow and retry
        // (always at least doubling, so a misbehaving libc cannot loop forever).
        let wanted = (count.max(0) as usize).max(buffer.len() * 2);
        if wanted > 65536 {
            return Err(std::io::Error::other("getgrouplist: too many groups"));
        }
        buffer.resize(wanted, 0);
    }
}

/// Caps a group list at `max` entries for `setgroups`, keeping `primary` first
/// when present. Apple's `setgroups` rejects more than `NGROUPS_MAX` (16)
/// entries with EINVAL, while `getgrouplist` can return more.
#[cfg_attr(not(target_vendor = "apple"), allow(dead_code))]
fn cap_groups(mut groups: Vec<libc::gid_t>, primary: libc::gid_t, max: usize) -> Vec<libc::gid_t> {
    if let Some(position) = groups.iter().position(|&group| group == primary) {
        let group = groups.remove(position);
        groups.insert(0, group);
    }
    groups.truncate(max);
    groups
}

/// Makes `command` run as `user` instead of root, with the user's own groups.
///
/// std applies `.uid()/.gid()` before `pre_exec` hooks, and root's supplementary
/// groups (wheel etc.) survive a plain uid/gid switch. So the whole drop happens
/// in one `pre_exec` hook, in the only safe order: supplementary groups first
/// (needs root), then gid, then uid (after which no further change is possible).
/// The group list is resolved before fork (see [`group_list`]); the hook itself
/// only issues the `setgroups`, `setgid` and `setuid` syscalls, which are
/// async-signal-safe and do not allocate.
pub fn drop_privileges(
    command: &mut Command,
    user: &str,
    uid: u32,
    gid: u32,
    home: &std::path::Path,
) -> std::io::Result<()> {
    let user_name = std::ffi::CString::new(user)
        .map_err(|_| std::io::Error::other("user name contains NUL"))?;
    let groups = group_list(&user_name, gid)?;
    #[cfg(target_vendor = "apple")]
    let groups = {
        // SAFETY: sysconf has no preconditions.
        let max = unsafe { libc::sysconf(libc::_SC_NGROUPS_MAX) };
        // Fallback: NGROUPS_MAX from <sys/syslimits.h> (not exported by libc).
        let max = if max > 0 { max as usize } else { 16 };
        cap_groups(groups, gid as libc::gid_t, max)
    };
    let (uid, gid) = (Uid::from_raw(uid), Gid::from_raw(gid));
    // SAFETY: the hook runs between fork and exec and calls only
    // async-signal-safe functions (setgroups, setgid, setuid) on data that was
    // fully prepared before the fork; it neither allocates nor takes locks.
    unsafe {
        command.pre_exec(move || {
            // `len as _`: the length parameter is size_t on Linux, c_int on Apple.
            if libc::setgroups(groups.len() as _, groups.as_ptr()) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            setgid(gid)?;
            setuid(uid)?;
            Ok(())
        });
    }
    command
        .env("HOME", home)
        .env("USER", user)
        .env("LOGNAME", user);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::cap_groups;

    #[test]
    fn cap_groups_keeps_primary_first_and_truncates() {
        let groups: Vec<libc::gid_t> = (1..=20).collect();
        let capped = cap_groups(groups, 18, 16);
        assert_eq!(capped.len(), 16);
        assert_eq!(capped[0], 18);
        assert_eq!(&capped[1..], &(1..=15).collect::<Vec<_>>()[..]);
    }

    #[test]
    fn cap_groups_leaves_short_list_unchanged_when_primary_absent() {
        assert_eq!(cap_groups(vec![5, 6, 7], 1, 16), vec![5, 6, 7]);
    }
}
