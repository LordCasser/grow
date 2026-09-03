//! Windows user-scoped IPC security. Creation installs the DACL before any
//! bearer token is written; discovery only validates, never changes an ACL.

use std::fs::File;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::path::Path;

use windows::Win32::Foundation::{CloseHandle, HANDLE, HLOCAL, LocalFree};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo,
    SDDL_REVISION_1, SE_FILE_OBJECT,
};
use windows::Win32::Security::{
    ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, DACL_SECURITY_INFORMATION, GetAce, GetTokenInformation,
    OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, SECURITY_ATTRIBUTES, TOKEN_QUERY,
    TOKEN_USER, TokenUser,
};
use windows::Win32::Storage::FileSystem::{
    CREATE_NEW, CreateDirectoryW, CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE, FILE_SHARE_READ,
    FILE_SHARE_WRITE,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::core::{PCWSTR, PWSTR};

fn denied(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, error.to_string())
}

fn sid_string(sid: PSID) -> io::Result<String> {
    let mut text = PWSTR::null();
    unsafe {
        ConvertSidToStringSidW(sid, &mut text).map_err(denied)?;
        let result = text.to_string().map_err(denied);
        let _ = LocalFree(Some(HLOCAL(text.0.cast())));
        result
    }
}

fn current_user_sid() -> io::Result<String> {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).map_err(denied)?;
        let mut size = 0;
        let _ = GetTokenInformation(token, TokenUser, None, 0, &mut size);
        // TOKEN_USER contains pointers; use aligned storage, not Vec<u8>.
        let mut buffer = vec![0usize; (size as usize).div_ceil(std::mem::size_of::<usize>())];
        let result = GetTokenInformation(
            token,
            TokenUser,
            Some(buffer.as_mut_ptr().cast()),
            size,
            &mut size,
        );
        let _ = CloseHandle(token);
        result.map_err(denied)?;
        sid_string((*(buffer.as_ptr().cast::<TOKEN_USER>())).User.Sid)
    }
}

pub(crate) struct UserSecurityAttributes {
    descriptor: PSECURITY_DESCRIPTOR,
    attributes: SECURITY_ATTRIBUTES,
}

impl UserSecurityAttributes {
    pub(crate) fn new() -> io::Result<Self> {
        let sid = current_user_sid()?;
        // Bind owner and access to TokenUser, not TokenOwner (which can be a
        // group). Children of private directories inherit the same user ACL.
        let sddl: Vec<u16> = format!("O:{sid}D:P(A;OICI;GA;;;{sid})\0")
            .encode_utf16()
            .collect();
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(sddl.as_ptr()),
                SDDL_REVISION_1,
                &mut descriptor,
                None,
            )
            .map_err(denied)?;
        }
        Ok(Self {
            descriptor,
            attributes: SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: descriptor.0,
                bInheritHandle: false.into(),
            },
        })
    }

    pub(crate) fn as_mut_ptr(&mut self) -> *mut std::ffi::c_void {
        (&mut self.attributes as *mut SECURITY_ATTRIBUTES).cast()
    }
}

impl Drop for UserSecurityAttributes {
    fn drop(&mut self) {
        unsafe {
            let _ = LocalFree(Some(HLOCAL(self.descriptor.0)));
        }
    }
}

/// canonicalize the existing parent with std (not dunce), preserving Windows
/// verbatim/UNC prefixes for direct Win32 calls including long GROW_HOME paths.
pub(crate) fn wide_path(path: &Path) -> io::Result<Vec<u16>> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("IPC path has no parent"))?;
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::other("IPC path has no filename"))?;
    Ok(std::fs::canonicalize(parent)?
        .join(name)
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect())
}

pub(crate) fn create_private_file(path: &Path) -> io::Result<File> {
    let security = UserSecurityAttributes::new()?;
    let path = wide_path(path)?;
    let handle = unsafe {
        CreateFileW(
            PCWSTR(path.as_ptr()),
            0xc0000000,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            Some(&security.attributes),
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OPEN_REPARSE_POINT,
            None,
        )
    }
    .map_err(|error| io::Error::from_raw_os_error(error.code().0 & 0xffff))?;
    Ok(unsafe { File::from_raw_handle(handle.0) })
}

pub(crate) fn open_private_file(path: &Path) -> io::Result<File> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0 | FILE_FLAG_BACKUP_SEMANTICS.0)
        .open(path)?;
    verify_private_file(&file)?;
    Ok(file)
}

pub(crate) fn create_private_directory(path: &Path) -> io::Result<()> {
    let security = UserSecurityAttributes::new()?;
    let wide = wide_path(path)?;
    match unsafe { CreateDirectoryW(PCWSTR(wide.as_ptr()), Some(&security.attributes)) } {
        Ok(()) => {}
        Err(error) if error.code().0 & 0xffff == 183 => {} // ERROR_ALREADY_EXISTS
        Err(error) => return Err(io::Error::from_raw_os_error(error.code().0 & 0xffff)),
    }
    let directory = open_private_file(path)?;
    if !directory.metadata()?.is_dir() {
        return Err(denied("IPC directory is not a directory"));
    }
    Ok(())
}

pub(crate) fn verify_private_file(file: &File) -> io::Result<()> {
    if file.metadata()?.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
        return Err(denied("IPC files must not be reparse points"));
    }
    let user = current_user_sid()?;
    let mut owner = PSID::default();
    let mut acl: *mut ACL = std::ptr::null_mut();
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    let result = unsafe {
        GetSecurityInfo(
            HANDLE(file.as_raw_handle()),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            Some(&mut owner),
            None,
            Some(&mut acl),
            None,
            Some(&mut descriptor),
        )
    };
    if result.0 != 0 {
        return Err(io::Error::from_raw_os_error(result.0 as i32));
    }
    let checked = (|| {
        if owner.0.is_null() || acl.is_null() || sid_string(owner)? != user {
            return Err(denied("IPC owner/DACL does not match the current user"));
        }
        unsafe {
            if (*acl).AceCount == 0 {
                return Err(denied("IPC DACL has no user grant"));
            }
            for index in 0..u32::from((*acl).AceCount) {
                let mut ace = std::ptr::null_mut();
                GetAce(acl, index, &mut ace).map_err(denied)?;
                let header = &*ace.cast::<ACE_HEADER>();
                if header.AceType != 0
                    || usize::from(header.AceSize) < std::mem::size_of::<ACCESS_ALLOWED_ACE>()
                {
                    return Err(denied("IPC DACL contains an unsupported ACE"));
                }
                let allowed = &*ace.cast::<ACCESS_ALLOWED_ACE>();
                // Accept only ordinary allow ACEs for exactly this user.
                if allowed.Header.AceType != 0
                    || sid_string(PSID(std::ptr::addr_of!(allowed.SidStart).cast_mut().cast()))?
                        != user
                {
                    return Err(denied("IPC DACL grants another principal access"));
                }
            }
        }
        Ok(())
    })();
    unsafe {
        let _ = LocalFree(Some(HLOCAL(descriptor.0)));
    }
    checked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_creation_and_read_only_acl_validation() {
        let root = tempfile::tempdir().unwrap();
        let private = root.path().join("private");
        create_private_directory(&private).unwrap();
        let path = private.join("manifest.json");
        drop(create_private_file(&path).unwrap());
        let before = std::fs::metadata(&path).unwrap().last_write_time();
        for _ in 0..5 {
            open_private_file(&path).unwrap();
        }
        assert_eq!(std::fs::metadata(&path).unwrap().last_write_time(), before);
        assert_eq!(
            create_private_file(&path).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
    }

    #[test]
    fn discovery_rejects_everyone_grant_without_repairing_it() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("insecure.json");
        let sid = current_user_sid().unwrap();
        let sddl: Vec<u16> = format!("O:{sid}D:P(A;;GA;;;WD)\0").encode_utf16().collect();
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(sddl.as_ptr()),
                SDDL_REVISION_1,
                &mut descriptor,
                None,
            )
            .unwrap();
        }
        let attributes = UserSecurityAttributes {
            descriptor,
            attributes: SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: descriptor.0,
                bInheritHandle: false.into(),
            },
        };
        let wide = wide_path(&path).unwrap();
        let handle = unsafe {
            CreateFileW(
                PCWSTR(wide.as_ptr()),
                0xc0000000,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                Some(&attributes.attributes),
                CREATE_NEW,
                FILE_ATTRIBUTE_NORMAL,
                None,
            )
            .unwrap()
        };
        drop(unsafe { File::from_raw_handle(handle.0) });
        for _ in 0..2 {
            assert_eq!(
                open_private_file(&path).unwrap_err().kind(),
                io::ErrorKind::PermissionDenied
            );
        }
    }
}
