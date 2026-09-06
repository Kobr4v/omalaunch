// SPDX-License-Identifier: GPL-3.0-or-later
//! memfd patched-runtime runner.
//!
//! Ports `binfmt-bypass/lib.cpp`: copy the ELF runtime into a memfd with the
//! AppImage magic bytes zeroed (so the kernel binfmt handler does not
//! re-enter us), then `fexecve` it as a subprocess with the redirect
//! environment set. `unsafe` is confined to this module; every block carries
//! a `SAFETY` justification.

use anyhow::{bail, Context, Result};
use goblin::elf::header::EI_CLASS;
use goblin::elf::program_header::PT_INTERP;
use oma_appimage::inspect::elf_payload_offset;
use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::io::{FromRawFd, RawFd};
use std::path::{Path, PathBuf};

pub const EXIT_FAILURE: i32 = 0xff;

/// Send SIGTERM to a pid. Used by `quit` to stop a running GUI instance.
/// Returns true when the signal was delivered.
pub fn signal_quit(pid: i32) -> bool {
    // SAFETY: kill() with SIGTERM on an observed pid; a reaped pid fails
    // harmlessly with ESRCH. No memory safety implications.
    unsafe { libc::kill(pid, libc::SIGTERM) == 0 }
}

/// Copy the first `size` bytes of `path`, zeroing the AppImage magic at 8..11.
pub fn patched_runtime(path: &Path, size: usize) -> Result<Vec<u8>> {
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    if bytes.len() < size {
        bail!("runtime larger than file: {}", path.display());
    }
    let mut runtime = bytes[..size].to_vec();
    if runtime.len() < 11 {
        bail!("runtime too small: {}", path.display());
    }
    // SAFETY: bounds checked above; mirrors copy_and_patch_runtime().
    runtime[8..11].fill(0);
    Ok(runtime)
}

/// Absolute argv for the child: argv[0] is the AppImage path itself.
/// Nul bytes cannot appear in exec argv; offending args are rejected.
pub fn child_argv(appimage: &Path, extra: &[String]) -> Result<Vec<CString>> {
    let mut argv = Vec::with_capacity(extra.len() + 1);
    argv.push(CString::new(appimage.as_os_str().as_bytes()).context("argv[0] contains nul")?);
    for arg in extra {
        argv.push(CString::new(arg.as_str()).context("arg contains nul")?);
    }
    Ok(argv)
}

fn is_32bit(bytes: &[u8]) -> bool {
    bytes.get(EI_CLASS) == Some(&1)
}

fn is_statically_linked(bytes: &[u8]) -> bool {
    match goblin::elf::Elf::parse(bytes) {
        Ok(elf) => !elf.program_headers.iter().any(|ph| ph.p_type == PT_INTERP),
        Err(_) => true,
    }
}

fn preload_lib_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let name = if cfg!(target_pointer_width = "32") {
        "libomalaunch_preload32.so"
    } else {
        "libomalaunch_preload.so"
    };
    let candidate = dir.join(name);
    candidate.is_file().then_some(candidate)
}

fn write_all(fd: RawFd, bytes: &[u8]) -> Result<()> {
    // SAFETY: fd is a valid owned memfd; File takes ownership for the scope
    // and closes it on drop. No other owner exists.
    let mut file = unsafe { std::fs::File::from_raw_fd(fd) };
    use std::io::Write;
    file.write_all(bytes).context("write memfd")?;
    Ok(())
}

static CHILD_PID: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

extern "C" fn forward_signal(sig: libc::c_int) {
    let pid = CHILD_PID.load(std::sync::atomic::Ordering::SeqCst);
    if pid > 0 {
        // SAFETY: kill() on our own forked child pid with the received
        // signal number; async-signal-safe.
        unsafe {
            libc::kill(pid, sig);
        }
    }
}

fn install_forwarding() {
    for sig in 1..32 {
        // SAFETY: the C signal API takes the handler as an integer; the
        // handler itself only calls async-signal-safe kill() on a known pid.
        unsafe {
            libc::signal(sig, forward_signal as *const () as libc::sighandler_t);
        }
    }
}

fn child_status_to_code(status: i32) -> i32 {
    if libc::WIFEXITED(status) {
        libc::WEXITSTATUS(status)
    } else if libc::WIFSIGNALED(status) {
        128 + libc::WTERMSIG(status)
    } else {
        EXIT_FAILURE
    }
}

/// Run `appimage` with `extra` args through the patched-runtime bypass.
/// Never returns on success (child replaces us via fork+fexecve semantics);
/// returns the child's exit code.
pub fn bypass(appimage: &Path, extra: &[String]) -> Result<i32> {
    let size = elf_payload_offset(appimage).map_err(|e| anyhow::anyhow!("{e}"))? as usize;
    let runtime = patched_runtime(appimage, size)?;

    // SAFETY: memfd_create with a static name; returns an owned fd or -1.
    let fd: RawFd = unsafe { libc::memfd_create(c"omalaunch-runtime".as_ptr(), libc::MFD_CLOEXEC) };
    if fd < 0 {
        bail!("memfd_create failed");
    }
    write_all(fd, &runtime)?;

    let abs = std::fs::canonicalize(appimage)
        .with_context(|| format!("realpath {}", appimage.display()))?;
    if !is_statically_linked(&runtime) {
        if let Some(lib) = preload_lib_path() {
            std::env::set_var("LD_PRELOAD", &lib);
        }
    }
    std::env::set_var("REDIRECT_APPIMAGE", &abs);
    std::env::set_var("TARGET_APPIMAGE", &abs);
    let _ = is_32bit(&runtime);

    // SAFETY: fork in a single-threaded context (called before any threads
    // spawn in this helper binary). Child only calls async-signal-safe
    // fexecve or _exit.
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        bail!("fork failed");
    }
    if pid == 0 {
        let argv = child_argv(&abs, extra)?;
        let mut argv_ptrs: Vec<*const libc::c_char> = argv.iter().map(|a| a.as_ptr()).collect();
        argv_ptrs.push(std::ptr::null());
        let env: Vec<CString> = std::env::vars_os()
            .filter_map(|(k, v)| {
                let mut pair = k.as_bytes().to_vec();
                if pair.contains(&0) {
                    return None;
                }
                pair.push(b'=');
                pair.extend_from_slice(v.as_bytes());
                CString::new(pair).ok()
            })
            .collect();
        let mut env_ptrs: Vec<*const libc::c_char> = env.iter().map(|e| e.as_ptr()).collect();
        env_ptrs.push(std::ptr::null());
        // SAFETY: argv/env are nul-terminated pointer arrays living past the call.
        unsafe {
            libc::fexecve(fd, argv_ptrs.as_ptr(), env_ptrs.as_ptr());
            libc::_exit(EXIT_FAILURE);
        }
    }
    CHILD_PID.store(pid, std::sync::atomic::Ordering::SeqCst);
    install_forwarding();
    let mut status = 0;
    // SAFETY: waiting on our own direct child.
    unsafe {
        libc::waitpid(pid, &mut status, 0);
    }
    // SAFETY: parent no longer needs the memfd after the child exec'd.
    unsafe {
        libc::close(fd);
    }
    Ok(child_status_to_code(status))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stub(ai: u8, extra_phdrs: &[(u32, u64, u64)]) -> Vec<u8> {
        let mut img = vec![0u8; 64 + 56 * extra_phdrs.len().max(1)];
        img[0..4].copy_from_slice(b"\x7fELF");
        img[4] = 2;
        img[5] = 1;
        img[8] = 0x41;
        img[9] = 0x49;
        img[10] = ai;
        img[16..18].copy_from_slice(&3u16.to_le_bytes());
        img[18..20].copy_from_slice(&62u16.to_le_bytes());
        img[32..40].copy_from_slice(&64u64.to_le_bytes());
        img[52..54].copy_from_slice(&64u16.to_le_bytes());
        img[54..56].copy_from_slice(&56u16.to_le_bytes());
        img[56..58].copy_from_slice(&(extra_phdrs.len().max(1) as u16).to_le_bytes());
        for (i, (ptype, off, filesz)) in extra_phdrs.iter().enumerate() {
            let mut p = vec![0u8; 56];
            p[0..4].copy_from_slice(&ptype.to_le_bytes());
            p[8..16].copy_from_slice(&off.to_le_bytes());
            p[32..40].copy_from_slice(&filesz.to_le_bytes());
            p[40..48].copy_from_slice(&filesz.to_le_bytes());
            img[64 + 56 * i..64 + 56 * (i + 1)].copy_from_slice(&p);
        }
        if extra_phdrs.is_empty() {
            let mut p = vec![0u8; 56];
            p[0..4].copy_from_slice(&1u32.to_le_bytes());
            p[32..40].copy_from_slice(&512u64.to_le_bytes());
            p[40..48].copy_from_slice(&512u64.to_le_bytes());
            img[64..120].copy_from_slice(&p);
            img.resize(512, 0);
        }
        img
    }

    #[test]
    fn magic_zeroed_prefix_kept() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("oma-bp-{}", std::process::id()));
        let mut img = stub(0x02, &[]);
        img.extend_from_slice(b"PAYLOAD");
        std::fs::write(&path, &img).expect("write");
        let patched = patched_runtime(&path, 512).expect("patch");
        assert_eq!(patched.len(), 512);
        assert_eq!(&patched[0..4], b"\x7fELF");
        assert_eq!(&patched[8..11], &[0, 0, 0]);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn argv_shape() {
        let argv = child_argv(
            Path::new("/a/App.AppImage"),
            &["--x".to_string(), "y".to_string()],
        )
        .expect("valid");
        assert_eq!(argv.len(), 3);
        assert_eq!(argv[0].to_str().expect("str"), "/a/App.AppImage");
    }

    #[test]
    fn argv_rejects_nul() {
        assert!(child_argv(Path::new("/a/App.AppImage"), &["a\0b".to_string()]).is_err());
    }

    #[test]
    fn quit_dead_pid_fails_cleanly() {
        assert!(!signal_quit(2_147_483_000));
    }

    #[test]
    fn static_vs_dynamic() {
        assert!(is_statically_linked(&stub(0x02, &[])));
        assert!(!is_statically_linked(&stub(0x02, &[(3, 0, 16)])));
    }

    #[test]
    fn too_small_runtime_rejected() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("oma-bp-small-{}", std::process::id()));
        std::fs::write(&path, b"\x7fELFtiny").expect("write");
        assert!(patched_runtime(&path, 512).is_err());
        std::fs::remove_file(&path).ok();
    }
}
