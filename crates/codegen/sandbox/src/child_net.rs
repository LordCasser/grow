//! Seccomp: child network filter (pre_exec) and process-wide namespace lockdown.

#[cfg(target_os = "linux")]
mod child_network {
    use libc::sock_filter;

    pub(super) const SECCOMP_RET_ALLOW: u32 = 0x7fff_0000;
    pub(super) const SECCOMP_RET_ERRNO: u32 = 0x0005_0000;
    pub(super) const EPERM_VAL: u32 = 1;
    pub(super) const OFF_NR: u32 = 0;
    pub(super) const OFF_ARCH: u32 = 4;

    #[cfg(target_arch = "x86_64")]
    pub(super) const EXPECTED_ARCH: u32 = 0xc000_003e; // AUDIT_ARCH_X86_64
    #[cfg(target_arch = "aarch64")]
    pub(super) const EXPECTED_ARCH: u32 = 0xc000_00b7; // AUDIT_ARCH_AARCH64
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    pub(super) const EXPECTED_ARCH: u32 = 0;

    #[cfg(target_arch = "x86_64")]
    pub(super) const X32_SYSCALL_BIT: u32 = 0x4000_0000;

    const BLOCKED_SYSCALLS: &[u32] = &[
        libc::SYS_connect as u32,
        libc::SYS_bind as u32,
        libc::SYS_sendto as u32,
        libc::SYS_sendmsg as u32,
        // A datagram destination can be supplied independently in every
        // mmsghdr, so blocking sendmsg without sendmmsg leaves a native UDP
        // egress bypass.
        libc::SYS_sendmmsg as u32,
        libc::SYS_listen as u32,
        libc::SYS_accept as u32,
        libc::SYS_accept4 as u32,
        // io_uring can perform network operations after the filter has admitted
        // the setup call, so all three entry points are denied as a unit.
        libc::SYS_io_uring_setup as u32,
        libc::SYS_io_uring_enter as u32,
        libc::SYS_io_uring_register as u32,
    ];

    const FILTER_LEN: usize =
        5 + BLOCKED_SYSCALLS.len() * 2 + if cfg!(target_arch = "x86_64") { 2 } else { 0 };

    fn stmt(code: u32, k: u32) -> sock_filter {
        sock_filter {
            code: code as u16,
            jt: 0,
            jf: 0,
            k,
        }
    }

    fn jump(code: u32, k: u32, jt: u8, jf: u8) -> sock_filter {
        sock_filter {
            code: code as u16,
            jt,
            jf,
            k,
        }
    }

    /// Classic BPF child-network filter.
    ///
    /// The audit architecture is checked before the syscall number. An
    /// unexpected/compat architecture is denied rather than interpreting its
    /// syscall table as the native one. x86_64 additionally rejects the x32
    /// syscall marker before exact syscall comparisons.
    pub(super) fn build_child_network_filter() -> [sock_filter; FILTER_LEN] {
        use libc::{BPF_ABS, BPF_JEQ, BPF_JMP, BPF_JSET, BPF_K, BPF_LD, BPF_RET, BPF_W};

        let empty = sock_filter {
            code: 0,
            jt: 0,
            jf: 0,
            k: 0,
        };
        let mut filter = [empty; FILTER_LEN];
        let mut len = 0;
        macro_rules! push {
            ($instruction:expr) => {{
                filter[len] = $instruction;
                len += 1;
            }};
        }

        push!(stmt(BPF_LD | BPF_W | BPF_ABS, OFF_ARCH));
        push!(jump(BPF_JMP | BPF_JEQ | BPF_K, EXPECTED_ARCH, 1, 0));
        push!(stmt(BPF_RET | BPF_K, SECCOMP_RET_ERRNO | EPERM_VAL));
        push!(stmt(BPF_LD | BPF_W | BPF_ABS, OFF_NR));
        #[cfg(target_arch = "x86_64")]
        {
            push!(jump(BPF_JMP | BPF_JSET | BPF_K, X32_SYSCALL_BIT, 0, 1));
            push!(stmt(BPF_RET | BPF_K, SECCOMP_RET_ERRNO | EPERM_VAL));
        }
        for &syscall in BLOCKED_SYSCALLS {
            push!(jump(BPF_JMP | BPF_JEQ | BPF_K, syscall, 0, 1));
            push!(stmt(BPF_RET | BPF_K, SECCOMP_RET_ERRNO | EPERM_VAL));
        }
        push!(stmt(BPF_RET | BPF_K, SECCOMP_RET_ALLOW));
        debug_assert_eq!(len, FILTER_LEN);
        filter
    }

    #[cfg(test)]
    pub(super) fn blocked_syscalls() -> &'static [u32] {
        BLOCKED_SYSCALLS
    }
}

#[cfg(target_os = "linux")]
mod ns_lockdown {
    use libc::sock_filter;

    pub(super) const SECCOMP_RET_ALLOW: u32 = 0x7fff_0000;
    pub(super) const SECCOMP_RET_ERRNO: u32 = 0x0005_0000;
    pub(super) const EPERM_VAL: u32 = 1;
    /// ENOSYS: libc treats clone3 as unavailable and falls back to legacy clone.
    pub(super) const ENOSYS_VAL: u32 = libc::ENOSYS as u32;
    #[cfg(target_arch = "x86_64")]
    pub(super) const X32_SYSCALL_BIT: u32 = 0x4000_0000;

    pub(super) const OFF_NR: u32 = 0;
    pub(super) const OFF_ARCH: u32 = 4;
    pub(super) const OFF_ARGS0_LO: u32 = 16; // LE low half of args[0]

    #[cfg(target_arch = "x86_64")]
    pub(super) const EXPECTED_ARCH: u32 = 0xc000_003e; // AUDIT_ARCH_X86_64
    #[cfg(target_arch = "aarch64")]
    pub(super) const EXPECTED_ARCH: u32 = 0xc000_00b7; // AUDIT_ARCH_AARCH64
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    pub(super) const EXPECTED_ARCH: u32 = 0;

    pub(super) const CLONE_NAMESPACE_BITS: u32 = (libc::CLONE_NEWNS as u32)
        | (libc::CLONE_NEWCGROUP as u32)
        | (libc::CLONE_NEWUTS as u32)
        | (libc::CLONE_NEWIPC as u32)
        | (libc::CLONE_NEWUSER as u32)
        | (libc::CLONE_NEWPID as u32)
        | (libc::CLONE_NEWNET as u32)
        | (libc::CLONE_NEWTIME as u32);

    /// Linux `clone3` (arch-portable number; not always exported by libc).
    pub(super) const SYS_CLONE3: u32 = 435;

    fn stmt(code: u32, k: u32) -> sock_filter {
        sock_filter {
            code: code as u16,
            jt: 0,
            jf: 0,
            k,
        }
    }

    fn jump(code: u32, k: u32, jt: u8, jf: u8) -> sock_filter {
        sock_filter {
            code: code as u16,
            jt,
            jf,
            k,
        }
    }

    /// Classic BPF namespace lockdown.
    ///
    /// - `unshare` / `setns` / legacy `clone(CLONE_NEW*)` → EPERM
    /// - `clone3` → ENOSYS (flags live in a pointed-to struct classic BPF cannot
    ///   inspect; ENOSYS makes libc fall back to legacy clone for ordinary
    ///   spawn, while direct malicious clone3 cannot create namespaces)
    pub fn build_namespace_lockdown_filter() -> Vec<sock_filter> {
        use libc::{
            BPF_ABS, BPF_JEQ, BPF_JMP, BPF_JSET, BPF_K, BPF_LD, BPF_RET, BPF_W, SYS_clone,
            SYS_setns, SYS_unshare,
        };

        let mut f = Vec::with_capacity(22);
        f.push(stmt(BPF_LD | BPF_W | BPF_ABS, OFF_ARCH));
        f.push(jump(BPF_JMP | BPF_JEQ | BPF_K, EXPECTED_ARCH, 1, 0));
        f.push(stmt(BPF_RET | BPF_K, SECCOMP_RET_ERRNO | EPERM_VAL));
        f.push(stmt(BPF_LD | BPF_W | BPF_ABS, OFF_NR));
        #[cfg(target_arch = "x86_64")]
        {
            f.push(jump(BPF_JMP | BPF_JSET | BPF_K, X32_SYSCALL_BIT, 0, 1));
            f.push(stmt(BPF_RET | BPF_K, SECCOMP_RET_ERRNO | EPERM_VAL));
        }
        for sys in [SYS_unshare as u32, SYS_setns as u32] {
            f.push(jump(BPF_JMP | BPF_JEQ | BPF_K, sys, 0, 1));
            f.push(stmt(BPF_RET | BPF_K, SECCOMP_RET_ERRNO | EPERM_VAL));
        }
        f.push(jump(BPF_JMP | BPF_JEQ | BPF_K, SYS_CLONE3, 0, 1));
        f.push(stmt(BPF_RET | BPF_K, SECCOMP_RET_ERRNO | ENOSYS_VAL));
        f.push(jump(BPF_JMP | BPF_JEQ | BPF_K, SYS_clone as u32, 0, 3));
        f.push(stmt(BPF_LD | BPF_W | BPF_ABS, OFF_ARGS0_LO));
        f.push(jump(BPF_JMP | BPF_JSET | BPF_K, CLONE_NAMESPACE_BITS, 0, 1));
        f.push(stmt(BPF_RET | BPF_K, SECCOMP_RET_ERRNO | EPERM_VAL));
        f.push(stmt(BPF_RET | BPF_K, SECCOMP_RET_ALLOW));
        f
    }

    #[cfg(test)]
    pub fn filter_jeq_immediates(filter: &[sock_filter]) -> Vec<u32> {
        use libc::{BPF_JEQ, BPF_JMP, BPF_K};
        let jeq = (BPF_JMP | BPF_JEQ | BPF_K) as u16;
        filter
            .iter()
            .filter(|i| i.code == jeq)
            .map(|i| i.k)
            .collect()
    }

    pub fn install(filter: &mut [sock_filter]) -> std::io::Result<()> {
        use libc::{
            PR_SET_NO_NEW_PRIVS, SECCOMP_FILTER_FLAG_TSYNC, SECCOMP_SET_MODE_FILTER, SYS_seccomp,
            prctl, sock_fprog,
        };

        let prog = sock_fprog {
            len: filter.len() as u16,
            filter: filter.as_mut_ptr(),
        };

        // SAFETY: standard NO_NEW_PRIVS before seccomp.
        if unsafe { prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) } != 0 {
            return Err(std::io::Error::last_os_error());
        }

        // SAFETY: prog valid for the duration of the syscall.
        // rc: 0 ok; >0 TSYNC failing TID; -1 errno.
        let rc = unsafe {
            libc::syscall(
                SYS_seccomp,
                SECCOMP_SET_MODE_FILTER as libc::c_long,
                SECCOMP_FILTER_FLAG_TSYNC as libc::c_long,
                &prog as *const sock_fprog as *const libc::c_void,
            )
        };
        if rc == 0 {
            return Ok(());
        }
        if rc > 0 {
            return Err(std::io::Error::other(format!(
                "seccomp TSYNC failed: thread {rc} could not install filter"
            )));
        }
        Err(std::io::Error::last_os_error())
    }
}

/// Extra pipes deliberately mapped by Grow before the network restriction hook.
/// The mapping producers create OS pipes; callers cannot pass an arbitrary fd.
#[cfg(target_os = "linux")]
#[derive(Debug, Clone, Copy)]
pub enum ChildFdAllowance {
    StdioOnly,
    StateInputPipe,
    StateInputAndOutputPipes,
}

#[cfg(target_os = "linux")]
fn admit_child_descriptors(allowance: ChildFdAllowance) -> std::io::Result<()> {
    // CLOEXEC keeps the stdlib's private exec-error pipe alive until exec while
    // ensuring every unknown descriptor disappears from the new process image.
    let rc = unsafe {
        libc::syscall(
            libc::SYS_close_range,
            3 as libc::c_uint,
            libc::c_uint::MAX,
            libc::CLOSE_RANGE_CLOEXEC,
        )
    };
    if rc != 0 {
        return Err(std::io::Error::last_os_error());
    }

    let pipes: &[libc::c_int] = match allowance {
        ChildFdAllowance::StdioOnly => &[],
        ChildFdAllowance::StateInputPipe => &[3],
        ChildFdAllowance::StateInputAndOutputPipes => &[3, 4],
    };
    for &fd in pipes {
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
        if flags < 0 {
            return Err(std::io::Error::last_os_error());
        }
        if unsafe { libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) } < 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(())
}

/// # Safety
/// After fork / before exec, after any deliberate fd mappings.
#[cfg(target_os = "linux")]
pub unsafe fn install_child_network_filter(allowance: ChildFdAllowance) -> std::io::Result<()> {
    use libc::{PR_SET_NO_NEW_PRIVS, PR_SET_SECCOMP, SECCOMP_MODE_FILTER, prctl, sock_fprog};

    admit_child_descriptors(allowance)?;
    let mut filter = child_network::build_child_network_filter();
    let prog = sock_fprog {
        len: filter.len() as u16,
        filter: filter.as_mut_ptr(),
    };
    if unsafe { prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    if unsafe {
        prctl(
            PR_SET_SECCOMP,
            SECCOMP_MODE_FILTER as libc::c_ulong,
            &prog as *const _ as libc::c_ulong,
            0,
            0,
        )
    } != 0
    {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// Deny nested namespace creation on all threads (TSYNC).
/// Ordinary process creation uses legacy clone after clone3 returns ENOSYS.
///
/// # Safety
/// Process-wide; call after bwrap re-exec / at apply.
#[cfg(target_os = "linux")]
pub unsafe fn install_namespace_lockdown_filter() -> std::io::Result<()> {
    let mut filter = ns_lockdown::build_namespace_lockdown_filter();
    ns_lockdown::install(&mut filter)
}

#[cfg(not(target_os = "linux"))]
/// No-op on platforms where the Linux seccomp filter is unavailable.
///
/// # Safety
/// Kept unsafe to preserve the cross-platform process-setup contract; callers
/// must invoke it only at the same pre-exec boundary as the Linux variant.
pub unsafe fn install_child_network_filter() -> std::io::Result<()> {
    Ok(())
}

#[cfg(not(target_os = "linux"))]
/// No-op on platforms where the Linux namespace filter is unavailable.
///
/// # Safety
/// Kept unsafe to preserve the cross-platform process-setup contract; callers
/// must invoke it only at the same pre-exec boundary as the Linux variant.
pub unsafe fn install_namespace_lockdown_filter() -> std::io::Result<()> {
    Ok(())
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::child_network;
    use super::ns_lockdown::*;
    use libc::{SYS_clone, SYS_setns, SYS_unshare, sock_filter};

    #[test]
    fn restricted_child_exec_closes_connected_socket_and_keeps_state_pipes() {
        use std::io::{Read as _, Write as _};
        use std::net::{TcpListener, TcpStream, UdpSocket};
        use std::os::fd::{AsRawFd as _, FromRawFd as _, OwnedFd};
        use std::os::unix::process::CommandExt as _;
        use std::process::Command;

        const CHILD: &str = "GROW_SANDBOX_FD_EXEC_CHILD";
        const SOCKET_FD: &str = "GROW_SANDBOX_FD_EXEC_SOCKET";
        const UDP_FD: &str = "GROW_SANDBOX_FD_EXEC_UDP";
        const MODE: &str = "GROW_SANDBOX_FD_EXEC_MODE";
        if std::env::var_os(CHILD).is_some() {
            let socket_fd: libc::c_int = std::env::var(SOCKET_FD).unwrap().parse().unwrap();
            let udp_fd: libc::c_int = std::env::var(UDP_FD).unwrap().parse().unwrap();
            let message = [b'!'];
            if std::env::var(MODE).unwrap() == "control" {
                assert_eq!(
                    unsafe { libc::write(socket_fd, message.as_ptr().cast(), 1) },
                    1,
                    "the control spawn must really inherit the connected socket"
                );
                assert_eq!(
                    unsafe { libc::write(udp_fd, message.as_ptr().cast(), 1) },
                    1
                );
                return;
            }
            let mut input = [0u8; 1];
            assert_eq!(unsafe { libc::read(3, input.as_mut_ptr().cast(), 1) }, 1);
            assert_eq!(input, [b'x']);
            let output = [b'y'];
            assert_eq!(unsafe { libc::write(4, output.as_ptr().cast(), 1) }, 1);
            assert_eq!(
                unsafe { libc::write(socket_fd, message.as_ptr().cast(), 1) },
                -1,
                "a pre-connected socket must not survive exec"
            );
            assert_eq!(
                unsafe { libc::write(udp_fd, message.as_ptr().cast(), 1) },
                -1
            );
            return;
        }

        fn pipe() -> (OwnedFd, OwnedFd) {
            let mut fds = [0; 2];
            assert_eq!(unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) }, 0);
            (unsafe { OwnedFd::from_raw_fd(fds[0]) }, unsafe {
                OwnedFd::from_raw_fd(fds[1])
            })
        }

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        // F_DUPFD deliberately produces a non-CLOEXEC descriptor outside the
        // stdio/state-pipe range. It models a connected parent socket leaked by
        // an unrelated subsystem.
        let raw_socket = unsafe { libc::fcntl(client.as_raw_fd(), libc::F_DUPFD, 64) };
        assert!(raw_socket >= 64);
        let socket = unsafe { OwnedFd::from_raw_fd(raw_socket) };
        let udp_receiver = UdpSocket::bind("127.0.0.1:0").unwrap();
        let udp_sender = UdpSocket::bind("127.0.0.1:0").unwrap();
        udp_sender
            .connect(udp_receiver.local_addr().unwrap())
            .unwrap();
        let raw_udp = unsafe { libc::fcntl(udp_sender.as_raw_fd(), libc::F_DUPFD, 64) };
        assert!(raw_udp >= 64);
        let udp = unsafe { OwnedFd::from_raw_fd(raw_udp) };
        let child_exe = std::env::current_exe().unwrap();
        let child_test =
            "child_net::tests::restricted_child_exec_closes_connected_socket_and_keeps_state_pipes";
        // Prove the fixture is a real inherited connection. Otherwise a child
        // that never inherited it could make the restricted branch pass vacuously.
        let control = Command::new(&child_exe)
            .args(["--exact", child_test])
            .env(CHILD, "1")
            .env(MODE, "control")
            .env(SOCKET_FD, raw_socket.to_string())
            .env(UDP_FD, raw_udp.to_string())
            .status()
            .unwrap();
        assert!(control.success());
        server
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();
        let mut control_byte = [0u8; 1];
        server.read_exact(&mut control_byte).unwrap();
        assert_eq!(control_byte, [b'!']);
        udp_receiver
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();
        let mut udp_control = [0u8; 1];
        let (n, _) = udp_receiver.recv_from(&mut udp_control).unwrap();
        assert_eq!((n, udp_control), (1, [b'!']));

        let (state_in_read, state_in_write) = pipe();
        let (state_out_read, state_out_write) = pipe();
        let mut state_in_write = std::fs::File::from(state_in_write);
        let mut state_out_read = std::fs::File::from(state_out_read);
        state_in_write.write_all(b"x").unwrap();
        drop(state_in_write);

        let mut command = Command::new(child_exe);
        command
            .args(["--exact", child_test])
            .env(CHILD, "1")
            .env(MODE, "restricted")
            .env(SOCKET_FD, raw_socket.to_string())
            .env(UDP_FD, raw_udp.to_string());
        let input_fd = state_in_read.as_raw_fd();
        let output_fd = state_out_write.as_raw_fd();
        unsafe {
            // The real launch registers command_fds mappings before this hook.
            command.pre_exec(move || {
                if libc::dup2(input_fd, 3) < 0 || libc::dup2(output_fd, 4) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
            command.pre_exec(|| {
                super::install_child_network_filter(
                    super::ChildFdAllowance::StateInputAndOutputPipes,
                )
            });
        }

        let status = command.status().unwrap();
        assert!(status.success());
        drop(command);
        drop(state_out_write);
        let mut reply = [0u8; 1];
        state_out_read.read_exact(&mut reply).unwrap();
        assert_eq!(reply, [b'y']);
        drop(socket);
        drop(client);
        drop(udp);
        drop(udp_sender);
        server
            .set_read_timeout(Some(std::time::Duration::from_millis(100)))
            .unwrap();
        let mut leak = [0u8; 1];
        assert_eq!(server.read(&mut leak).unwrap(), 0);
    }

    fn child_filter_is_eperm(filter: &[sock_filter], arch: u32, nr: u32) -> bool {
        eval_filter(filter, arch, nr, 0)
            == (child_network::SECCOMP_RET_ERRNO | child_network::EPERM_VAL)
    }

    #[test]
    fn child_network_filter_denies_socket_syscalls_but_allows_unrelated_syscalls() {
        let filter = child_network::build_child_network_filter();
        let threats = [
            ("connect", libc::SYS_connect as u32),
            ("bind", libc::SYS_bind as u32),
            ("sendto", libc::SYS_sendto as u32),
            ("sendmsg", libc::SYS_sendmsg as u32),
            (
                "sendmmsg per-message UDP destination",
                libc::SYS_sendmmsg as u32,
            ),
            ("listen", libc::SYS_listen as u32),
            ("accept", libc::SYS_accept as u32),
            ("accept4", libc::SYS_accept4 as u32),
        ];

        for (name, nr) in threats {
            assert!(
                child_filter_is_eperm(&filter, child_network::EXPECTED_ARCH, nr),
                "{name} must be denied"
            );
        }
        assert_eq!(
            eval_filter(
                &filter,
                child_network::EXPECTED_ARCH,
                libc::SYS_read as u32,
                0,
            ),
            child_network::SECCOMP_RET_ALLOW,
            "an unrelated syscall must remain allowed"
        );
    }

    #[test]
    fn child_network_filter_denies_io_uring_async_io_bypass() {
        let filter = child_network::build_child_network_filter();
        let threats = [
            ("io_uring_setup", libc::SYS_io_uring_setup as u32),
            ("io_uring_enter", libc::SYS_io_uring_enter as u32),
            ("io_uring_register", libc::SYS_io_uring_register as u32),
        ];

        for (name, nr) in threats {
            assert!(
                child_filter_is_eperm(&filter, child_network::EXPECTED_ARCH, nr),
                "{name} must be denied"
            );
        }
        assert_eq!(
            child_network::blocked_syscalls().len(),
            11,
            "the deny table must contain the eight socket and three io_uring entry points"
        );
    }

    #[test]
    fn child_network_filter_denies_unexpected_arch_and_compat_socketcall() {
        const AUDIT_ARCH_I386: u32 = 0x4000_0003;
        const AUDIT_ARCH_ARM: u32 = 0x4000_0028;
        const I386_SYS_SOCKETCALL: u32 = 102;

        let filter = child_network::build_child_network_filter();
        let threats = [
            ("unknown audit arch", 0xdead_beef, libc::SYS_read as u32),
            (
                "i386 socketcall compat ABI",
                AUDIT_ARCH_I386,
                I386_SYS_SOCKETCALL,
            ),
            ("ARM compat ABI", AUDIT_ARCH_ARM, libc::SYS_connect as u32),
        ];

        for (name, arch, nr) in threats {
            assert!(
                child_filter_is_eperm(&filter, arch, nr),
                "{name} must fail closed before syscall-table interpretation"
            );
        }
    }

    #[test]
    #[cfg(target_arch = "x86_64")]
    fn child_network_filter_denies_x32_socket_number_mask_bypass() {
        let filter = child_network::build_child_network_filter();
        let threats = [
            libc::SYS_connect as u32 | child_network::X32_SYSCALL_BIT,
            libc::SYS_sendmsg as u32 | child_network::X32_SYSCALL_BIT,
            libc::SYS_sendmmsg as u32 | child_network::X32_SYSCALL_BIT,
            libc::SYS_read as u32 | child_network::X32_SYSCALL_BIT,
        ];

        for nr in threats {
            assert!(
                child_filter_is_eperm(&filter, child_network::EXPECTED_ARCH, nr),
                "x32-marked syscall {nr:#x} must be denied before exact comparisons"
            );
        }
    }

    /// Minimal classic-BPF interpreter over synthetic seccomp_data fields.
    fn eval_filter(filter: &[sock_filter], arch: u32, nr: u32, arg0_lo: u32) -> u32 {
        use libc::{BPF_ABS, BPF_JEQ, BPF_JMP, BPF_JSET, BPF_K, BPF_LD, BPF_RET, BPF_W};
        let mut pc = 0usize;
        let mut a = 0u32;
        for _ in 0..filter.len().saturating_mul(2) {
            let insn = &filter[pc];
            let op = insn.code as u32;
            if op == (BPF_LD | BPF_W | BPF_ABS) {
                a = match insn.k {
                    OFF_NR => nr,
                    OFF_ARCH => arch,
                    OFF_ARGS0_LO => arg0_lo,
                    _ => 0,
                };
                pc += 1;
            } else if op == (BPF_JMP | BPF_JEQ | BPF_K) {
                pc = if a == insn.k {
                    pc + 1 + insn.jt as usize
                } else {
                    pc + 1 + insn.jf as usize
                };
            } else if op == (BPF_JMP | BPF_JSET | BPF_K) {
                pc = if a & insn.k != 0 {
                    pc + 1 + insn.jt as usize
                } else {
                    pc + 1 + insn.jf as usize
                };
            } else if op == (BPF_RET | BPF_K) {
                return insn.k;
            } else {
                panic!("unsupported opcode {:#x} at {pc}", insn.code);
            }
            if pc >= filter.len() {
                panic!("pc out of range");
            }
        }
        panic!("filter did not RET");
    }

    fn is_allow(r: u32) -> bool {
        r == SECCOMP_RET_ALLOW
    }
    fn is_eperm(r: u32) -> bool {
        r == (SECCOMP_RET_ERRNO | EPERM_VAL)
    }
    fn is_enosys(r: u32) -> bool {
        r == (SECCOMP_RET_ERRNO | ENOSYS_VAL)
    }

    #[test]
    fn namespace_filter_targets_unshare_setns_clone3_and_clone() {
        let f = build_namespace_lockdown_filter();
        let jeqs = filter_jeq_immediates(&f);
        assert!(jeqs.contains(&(SYS_unshare as u32)), "{jeqs:?}");
        assert!(jeqs.contains(&(SYS_setns as u32)), "{jeqs:?}");
        assert!(jeqs.contains(&SYS_CLONE3), "{jeqs:?}");
        assert!(jeqs.contains(&(SYS_clone as u32)), "{jeqs:?}");
        assert!(jeqs.contains(&EXPECTED_ARCH), "{jeqs:?}");
    }

    #[test]
    fn bpf_eval_ordinary_clone_allowed_namespace_clone_denied() {
        let f = build_namespace_lockdown_filter();
        // Ordinary clone/fork flags (no NEW*)
        assert!(is_allow(eval_filter(
            &f,
            EXPECTED_ARCH,
            SYS_clone as u32,
            0x11 /* SIGCHLD | CLONE_VM-ish low bits without NEW* */
        )));
        assert!(is_eperm(eval_filter(
            &f,
            EXPECTED_ARCH,
            SYS_clone as u32,
            libc::CLONE_NEWUSER as u32
        )));
        assert!(is_eperm(eval_filter(
            &f,
            EXPECTED_ARCH,
            SYS_clone as u32,
            libc::CLONE_NEWNS as u32
        )));
    }

    #[test]
    fn bpf_eval_clone3_enosys_unshare_setns_eperm_read_allowed() {
        let f = build_namespace_lockdown_filter();
        assert!(is_enosys(eval_filter(&f, EXPECTED_ARCH, SYS_CLONE3, 0)));
        assert!(is_eperm(eval_filter(
            &f,
            EXPECTED_ARCH,
            SYS_unshare as u32,
            0
        )));
        assert!(is_eperm(eval_filter(
            &f,
            EXPECTED_ARCH,
            SYS_setns as u32,
            0
        )));
        assert!(is_allow(eval_filter(&f, EXPECTED_ARCH, 0, 0)));
    }

    #[test]
    fn bpf_eval_wrong_arch_and_x32_denied() {
        let f = build_namespace_lockdown_filter();
        assert!(is_eperm(eval_filter(&f, 0xdead_beef, SYS_clone as u32, 0)));
        #[cfg(target_arch = "x86_64")]
        {
            // x32: nr has high bit set
            assert!(is_eperm(eval_filter(
                &f,
                EXPECTED_ARCH,
                (SYS_unshare as u32) | X32_SYSCALL_BIT,
                0
            )));
        }
    }

    #[test]
    fn namespace_bits_cover_user_ns_and_mount_ns() {
        assert_ne!(CLONE_NAMESPACE_BITS & (libc::CLONE_NEWUSER as u32), 0);
        assert_ne!(CLONE_NAMESPACE_BITS & (libc::CLONE_NEWNS as u32), 0);
        assert_ne!(CLONE_NAMESPACE_BITS & (libc::CLONE_NEWNET as u32), 0);
    }

    #[test]
    fn filter_ends_with_allow() {
        let f = build_namespace_lockdown_filter();
        assert_eq!(f.last().unwrap().k, SECCOMP_RET_ALLOW);
    }
}
