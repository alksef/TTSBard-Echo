/* ==========================================================================
Windows ACL hardening for configuration files
========================================================================== */
//! Restricted DACLs for configuration files (roadmap 010, task 006).
//!
//! After any backend operation a configuration file (`settings.json`,
//! `windows.json`, their `.bak` backups, the `*.tmp` temporaries and the
//! `*.corrupt-*` quarantine copies) carries a DACL that grants access only
//! to:
//!
//! - the **current user** (full control);
//! - `SYSTEM` and the local **Administrators** group (full control).
//!
//! The DACL is written **protected** (`PROTECTED_DACL_SECURITY_INFORMATION`),
//! which discards inherited ACEs and blocks further inheritance — broad
//! inherited grants ("Everyone", "Authenticated Users", "Users") that the
//! parent directory chain propagates cannot reach the file contents, and no
//! other standard user can read or modify the file.
//!
//! ## Why SYSTEM and Administrators are included
//!
//! These are the same system subjects the default inherited DACL of a file
//! under the user profile carries. They are machine-level subjects, not
//! broad access groups: membership in Administrators already implies full
//! control of the machine, and SYSTEM is the operating system itself, so
//! including them does not weaken the property this hardening is about
//! (other *standard* users get no access). Keeping them preserves normal
//! Windows operation (backup, servicing, administrative maintenance) without
//! an ownership take-over, at no cost to the security goal.
//!
//! ## Where the restriction is applied
//!
//! - In the single write mechanism, [`crate::config::atomic`]: the DACL is
//!   applied to the temporary file *before* the rename (a rename moves the
//!   security descriptor, so the target inherits the restricted DACL) and to
//!   the `.bak` copy right after it is made. There a failure fails the whole
//!   write atomically: the previous target stays untouched and the temporary
//!   file is removed.
//! - In [`crate::config::recovery`]: the quarantined `*.corrupt-*` copy and
//!   an already existing file that loads fine (created by v0.1.0 with broad
//!   rights, possibly not rewritten until the next save) are restricted
//!   best-effort — a failure is logged and never blocks startup.
//!
//! ## API choice
//!
//! Implemented with the `windows` crate (Advapi32 via the
//! `Win32_Security_Authorization` feature): the current user SID is read
//! from the process token, the new ACL is built with `SetEntriesInAclW`
//! (explicit grants instead of a hand-computed ACL byte layout), and
//! `SetNamedSecurityInfoW` replaces the DACL in one step, protected against
//! inheritance. On other platforms this module is a no-op and none of the
//! Windows code is compiled; the added crate features (`Win32_Foundation`,
//! `Win32_Security`, `Win32_Security_Authorization`, `Win32_System_Threading`)
//! are Windows-only dependencies (see `Cargo.toml`).

use anyhow::Result;
use std::path::Path;

/// Restrict the DACL of `path` to the current user plus the Windows system
/// subjects, protected against inheritance (Windows). A no-op elsewhere.
///
/// The operation is idempotent: re-applying the restriction to an already
/// restricted file yields the same DACL and does not damage the file.
pub(crate) fn restrict_to_current_user(path: &Path) -> Result<()> {
    restrict_impl(path)
}

#[cfg(windows)]
fn restrict_impl(path: &Path) -> Result<()> {
    native::restrict(path)
}

#[cfg(not(windows))]
fn restrict_impl(_path: &Path) -> Result<()> {
    Ok(())
}

/* ==========================================================================
Win32 implementation (Windows only)
========================================================================== */
#[cfg(windows)]
mod native {
    use anyhow::{anyhow, bail, Context, Result};
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::Foundation::{
        CloseHandle, LocalFree, ERROR_SUCCESS, GENERIC_ALL, HANDLE, HLOCAL, WIN32_ERROR,
    };
    use windows::Win32::Security::Authorization::{
        SetEntriesInAclW, SetNamedSecurityInfoW, EXPLICIT_ACCESS_W, GRANT_ACCESS,
        NO_MULTIPLE_TRUSTEE, SE_FILE_OBJECT, TRUSTEE_IS_SID, TRUSTEE_IS_USER,
        TRUSTEE_IS_WELL_KNOWN_GROUP, TRUSTEE_TYPE, TRUSTEE_W,
    };
    use windows::Win32::Security::{
        CreateWellKnownSid, GetLengthSid, GetTokenInformation, TokenUser,
        WinBuiltinAdministratorsSid, WinLocalSystemSid, ACL, DACL_SECURITY_INFORMATION,
        PROTECTED_DACL_SECURITY_INFORMATION, PSID, SECURITY_MAX_SID_SIZE, TOKEN_QUERY, TOKEN_USER,
        WELL_KNOWN_SID_TYPE,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    /// An owned SID in a byte buffer, presented to the APIs as `PSID`.
    struct Sid {
        bytes: Vec<u8>,
    }

    impl Sid {
        fn as_psid(&self) -> PSID {
            PSID(self.bytes.as_ptr().cast_mut().cast())
        }
    }

    /// Replace the DACL of `path` with a protected ACL granting full control
    /// to the current user, SYSTEM, and Administrators (see module docs).
    pub(super) fn restrict(path: &Path) -> Result<()> {
        let wide = wide_path(path);

        let user = current_user_sid()?;
        let system = well_known_sid(WinLocalSystemSid)?;
        let administrators = well_known_sid(WinBuiltinAdministratorsSid)?;

        let entries = [
            full_control_grant(&user, TRUSTEE_IS_USER),
            full_control_grant(&system, TRUSTEE_IS_WELL_KNOWN_GROUP),
            full_control_grant(&administrators, TRUSTEE_IS_WELL_KNOWN_GROUP),
        ];
        let mut dacl: *mut ACL = std::ptr::null_mut();
        let result: WIN32_ERROR = unsafe { SetEntriesInAclW(Some(&entries), None, &mut dacl) };
        if result != ERROR_SUCCESS {
            return Err(anyhow!(
                "SetEntriesInAclW failed with Win32 error {}",
                result.0
            ));
        }

        // Replace the DACL and mark it protected: inherited ACEs from the
        // parent directory chain (including any broad group grants) are
        // dropped and will not reappear.
        let result = unsafe {
            SetNamedSecurityInfoW(
                PCWSTR::from_raw(wide.as_ptr()),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                None,
                None,
                Some(dacl as *const ACL),
                None,
            )
        };
        // The ACL was allocated by SetEntriesInAclW; free it either way.
        unsafe {
            let _ = LocalFree(Some(HLOCAL(dacl.cast())));
        }
        if result != ERROR_SUCCESS {
            return Err(anyhow!(
                "SetNamedSecurityInfoW failed with Win32 error {}",
                result.0
            ));
        }
        Ok(())
    }

    /// An `EXPLICIT_ACCESS` entry granting full control to `sid`.
    fn full_control_grant(sid: &Sid, trustee_type: TRUSTEE_TYPE) -> EXPLICIT_ACCESS_W {
        EXPLICIT_ACCESS_W {
            // GENERIC_ALL maps to FILE_ALL_ACCESS on file objects.
            grfAccessPermissions: GENERIC_ALL.0,
            grfAccessMode: GRANT_ACCESS,
            // Files have no children: no inheritance flags.
            grfInheritance: windows::Win32::Security::ACE_FLAGS(0),
            Trustee: TRUSTEE_W {
                pMultipleTrustee: std::ptr::null_mut(),
                MultipleTrusteeOperation: NO_MULTIPLE_TRUSTEE,
                TrusteeForm: TRUSTEE_IS_SID,
                TrusteeType: trustee_type,
                ptstrName: PWSTR(sid.as_psid().0.cast()),
            },
        }
    }

    /// The SID of the current user, read from the process token.
    fn current_user_sid() -> Result<Sid> {
        let mut token = HANDLE(std::ptr::null_mut());
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }
            .context("OpenProcessToken failed")?;
        let result = token_user_sid(token);
        unsafe {
            let _ = CloseHandle(token);
        }
        result
    }

    fn token_user_sid(token: HANDLE) -> Result<Sid> {
        // The first call only reports the required buffer size. The buffer is
        // `u64`-backed so the `TOKEN_USER` structure at its start (which
        // embeds a pointer) is properly aligned.
        let mut needed = 0u32;
        let _ = unsafe { GetTokenInformation(token, TokenUser, None, 0, &mut needed) };
        if needed == 0 {
            bail!("GetTokenInformation(TokenUser) could not determine the buffer size");
        }
        let mut buffer = vec![0u64; needed.div_ceil(size_of::<u64>() as u32) as usize];
        unsafe {
            GetTokenInformation(
                token,
                TokenUser,
                Some(buffer.as_mut_ptr().cast()),
                needed,
                &mut needed,
            )
        }
        .context("GetTokenInformation(TokenUser) failed")?;

        let user = unsafe { &*(buffer.as_ptr().cast::<TOKEN_USER>()) };
        let sid = user.User.Sid;
        if sid.is_invalid() {
            bail!("process token carries no user SID");
        }
        let length = unsafe { GetLengthSid(sid) } as usize;
        let mut bytes = vec![0u8; length];
        unsafe { std::ptr::copy_nonoverlapping(sid.0.cast::<u8>(), bytes.as_mut_ptr(), length) };
        Ok(Sid { bytes })
    }

    /// A well-known SID such as SYSTEM or Administrators.
    fn well_known_sid(kind: WELL_KNOWN_SID_TYPE) -> Result<Sid> {
        let mut bytes = vec![0u8; SECURITY_MAX_SID_SIZE as usize];
        let mut size = bytes.len() as u32;
        unsafe { CreateWellKnownSid(kind, None, Some(PSID(bytes.as_mut_ptr().cast())), &mut size) }
            .with_context(|| format!("CreateWellKnownSid failed for type {}", kind.0))?;
        bytes.truncate(size as usize);
        Ok(Sid { bytes })
    }

    fn wide_path(path: &Path) -> Vec<u16> {
        let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
        wide.push(0);
        wide
    }

    /* ------------------------------------------------------------------
    DACL inspection — compiled only for the Windows unit tests below.
    ------------------------------------------------------------------ */
    #[cfg(test)]
    pub(crate) mod inspection {
        use super::*;
        use std::ffi::c_void;
        use windows::core::BOOL;
        use windows::Win32::Security::Authorization::GetNamedSecurityInfoW;
        use windows::Win32::Security::{
            AclSizeInformation, GetAce, GetAclInformation, GetSecurityDescriptorControl,
            GetSecurityDescriptorDacl, WinAuthenticatedUserSid, WinBuiltinUsersSid, WinWorldSid,
            ACCESS_ALLOWED_ACE, ACL, ACL_SIZE_INFORMATION, PSECURITY_DESCRIPTOR,
            SECURITY_DESCRIPTOR_CONTROL, SE_DACL_PROTECTED,
        };

        /// The DACL of `path`, read back with `GetNamedSecurityInfoW`: the
        /// SID bytes of every ACE and whether the DACL is protected against
        /// inheritance.
        pub(crate) fn read_dacl(path: &Path) -> Result<(Vec<Vec<u8>>, bool)> {
            let wide = wide_path(path);
            let mut descriptor = PSECURITY_DESCRIPTOR(std::ptr::null_mut());
            let error = unsafe {
                GetNamedSecurityInfoW(
                    PCWSTR::from_raw(wide.as_ptr()),
                    SE_FILE_OBJECT,
                    DACL_SECURITY_INFORMATION,
                    None,
                    None,
                    None,
                    None,
                    &mut descriptor,
                )
            };
            if error != ERROR_SUCCESS {
                bail!("GetNamedSecurityInfoW failed with Win32 error {}", error.0);
            }
            // The returned descriptor must be freed either way.
            let outcome = describe_dacl(descriptor);
            unsafe {
                let _ = LocalFree(Some(HLOCAL(descriptor.0)));
            }
            outcome
        }

        fn describe_dacl(descriptor: PSECURITY_DESCRIPTOR) -> Result<(Vec<Vec<u8>>, bool)> {
            unsafe {
                let mut control = 0u16;
                let mut revision = 0u32;
                GetSecurityDescriptorControl(descriptor, &mut control, &mut revision)
                    .context("GetSecurityDescriptorControl failed")?;
                let protected = SECURITY_DESCRIPTOR_CONTROL(control).contains(SE_DACL_PROTECTED);

                let mut present = BOOL(0);
                let mut defaulted = BOOL(0);
                let mut dacl: *mut ACL = std::ptr::null_mut();
                GetSecurityDescriptorDacl(descriptor, &mut present, &mut dacl, &mut defaulted)
                    .context("GetSecurityDescriptorDacl failed")?;
                if !present.as_bool() || dacl.is_null() {
                    bail!("security descriptor carries no DACL");
                }

                let mut size = ACL_SIZE_INFORMATION::default();
                GetAclInformation(
                    dacl,
                    &mut size as *mut ACL_SIZE_INFORMATION as *mut c_void,
                    size_of::<ACL_SIZE_INFORMATION>() as u32,
                    AclSizeInformation,
                )
                .context("GetAclInformation failed")?;

                let mut sids = Vec::new();
                for index in 0..size.AceCount {
                    let mut ace: *mut c_void = std::ptr::null_mut();
                    GetAce(dacl, index, &mut ace).context("GetAce failed")?;
                    // `SidStart` marks where the SID begins inside the ACE.
                    let allowed = &*(ace.cast::<ACCESS_ALLOWED_ACE>());
                    let sid = PSID(std::ptr::addr_of!(allowed.SidStart).cast_mut().cast());
                    let length = GetLengthSid(sid) as usize;
                    let mut bytes = vec![0u8; length];
                    std::ptr::copy_nonoverlapping(sid.0.cast::<u8>(), bytes.as_mut_ptr(), length);
                    sids.push(bytes);
                }
                Ok((sids, protected))
            }
        }

        /// True when `sid_bytes` is one of the broad access groups whose ACEs
        /// must never appear on a configuration file: Everyone,
        /// Authenticated Users, Users.
        pub(crate) fn is_broad_group(sid_bytes: &[u8]) -> bool {
            [WinWorldSid, WinAuthenticatedUserSid, WinBuiltinUsersSid]
                .iter()
                .any(|kind| match well_known_sid(*kind) {
                    Ok(sid) => sid.bytes.as_slice() == sid_bytes,
                    Err(_) => false,
                })
        }

        /// SID bytes of the current user, for asserting the user's grant.
        pub(crate) fn current_user_sid_bytes() -> Vec<u8> {
            super::current_user_sid()
                .expect("current user SID from the process token")
                .bytes
        }
    }
}

/* ==========================================================================
Tests (Windows)
========================================================================== */
#[cfg(all(test, windows))]
mod windows_tests {
    use super::native::inspection::{current_user_sid_bytes, is_broad_group, read_dacl};
    use super::restrict_to_current_user;
    use crate::config::{atomic, recovery};
    use std::path::PathBuf;

    fn test_dir() -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("ttsbard-echo-acl-{}-{unique}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn has_no_broad_group_aces(sids: &[Vec<u8>]) -> bool {
        !sids.iter().any(|sid| is_broad_group(sid))
    }

    #[test]
    fn restricting_a_file_in_an_explicit_directory_succeeds_and_is_idempotent() {
        let dir = test_dir();
        let file = dir.join("settings.json");
        let content = r#"{"settings":"v1"}"#;
        std::fs::write(&file, content).unwrap();

        restrict_to_current_user(&file).unwrap();
        // The file itself is untouched and now carries a protected DACL with
        // a grant for the current user and no broad-group ACEs.
        assert_eq!(std::fs::read_to_string(&file).unwrap(), content);
        let (sids, protected) = read_dacl(&file).unwrap();
        assert!(
            protected,
            "the DACL must be protected against inherited ACEs"
        );
        assert!(
            has_no_broad_group_aces(&sids),
            "no Everyone / Authenticated Users / Users ACEs may remain: {sids:?}"
        );
        let user_sid = current_user_sid_bytes();
        assert!(
            sids.iter().any(|sid| sid.as_slice() == user_sid),
            "the current user must have an explicit grant: {sids:?}"
        );

        // Idempotent: applying the restriction again succeeds, keeps the
        // file intact and the same restricted DACL.
        restrict_to_current_user(&file).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), content);
        let (sids_again, protected_again) = read_dacl(&file).unwrap();
        assert_eq!(sids_again, sids);
        assert!(protected_again);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The write mechanism itself (roadmap 010, task 002) must produce
    /// restricted artifacts: the target, and the `.bak` backup created by a
    /// second write.
    #[test]
    fn written_config_files_and_backups_carry_restricted_dacls() {
        let dir = test_dir();
        let target = dir.join("windows.json");

        atomic::write_atomic(&target, "v1").unwrap();
        let (sids, protected) = read_dacl(&target).unwrap();
        assert!(protected);
        assert!(has_no_broad_group_aces(&sids));

        atomic::write_atomic(&target, "v2").unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "v2");
        let backup = dir.join("windows.json.bak");
        for path in [target.clone(), backup.clone()] {
            let (sids, protected) = read_dacl(&path).unwrap();
            assert!(protected, "{}: DACL must be protected", path.display());
            assert!(
                has_no_broad_group_aces(&sids),
                "{}: no broad-group ACEs may remain",
                path.display()
            );
        }
        // The previous version was preserved as the backup.
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), "v1");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// An already existing file that loads fine (a v0.1.0 file with broad
    /// rights) is closed when the backend processes it at load time, even
    /// though nothing was rewritten.
    #[test]
    fn loading_an_existing_config_closes_broad_inherited_rights() {
        let dir = test_dir();
        let file = dir.join("settings.json");
        std::fs::write(&file, r#"{"theme":"dark"}"#).unwrap();

        let value: serde_json::Value = recovery::load_with_recovery(&file, serde_json::Value::Null);
        assert_eq!(value["theme"], "dark");

        let (sids, protected) = read_dacl(&file).unwrap();
        assert!(protected);
        assert!(has_no_broad_group_aces(&sids));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The quarantined `*.corrupt-*` copy kept next to a damaged config is
    /// restricted like every other configuration artifact.
    #[test]
    fn quarantined_corrupt_copies_are_restricted() {
        let dir = test_dir();
        let file = dir.join("settings.json");
        std::fs::write(&file, "not-json").unwrap();

        let value: serde_json::Value = recovery::load_with_recovery(&file, serde_json::Value::Null);
        assert_eq!(value, serde_json::Value::Null);

        let corrupt: Vec<PathBuf> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.to_string_lossy().contains(".corrupt-"))
            .collect();
        assert_eq!(corrupt.len(), 1);
        let (sids, protected) = read_dacl(&corrupt[0]).unwrap();
        assert!(protected);
        assert!(has_no_broad_group_aces(&sids));

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
