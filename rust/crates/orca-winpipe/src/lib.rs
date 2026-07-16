//! Windows named-pipe **server** transport for orca-daemon, isolated behind a
//! safe API. The unsafe winapi FFI lives here so `orca-daemon` keeps
//! `unsafe_code = "forbid"` (the portable-pty precedent). Empty on non-Windows.
//!
//! Model: a byte-mode duplex pipe. [`NamedPipeListener::accept`] blocks in
//! `ConnectNamedPipe` until a client dials — the named-pipe analogue of
//! `UnixListener::accept`. Each accepted [`NamedPipeStream`] is `Read + Write`
//! and `try_clone`s (via `DuplicateHandle`) so the daemon can split a reader
//! thread from a writer thread exactly as it does for `UnixStream`.
//!
//! `ERROR_PIPE_BUSY` hardening: `bind()` creates the first instance (so the name
//! is claimed before "listening" is announced) and `accept()` pre-creates the
//! NEXT instance immediately after a client connects, before handing the
//! connected one to the caller. A dialing client therefore always finds a free
//! instance except during the single `CreateNamedPipeW` syscall of the pre-arm
//! (or a multi-client burst); the JS clients cover that residue with a bounded
//! EBUSY connect retry. The pre-arm deliberately happens AFTER `ConnectNamedPipe`
//! returns — creating it before blocking would let a client land on the idle
//! instance while the server waits on the other one, stalling that client until
//! a second dial arrives.
//!
//! Cross-compile-verified for x86_64-pc-windows; written to the winapi 0.3
//! signatures and the stable Win32 ABI.

#[cfg(windows)]
mod imp {
    use std::io::{self, Read, Write};
    use std::ptr;
    use winapi::ctypes::c_void;
    use winapi::shared::minwindef::{DWORD, FALSE, LPCVOID, LPVOID};
    use winapi::um::errhandlingapi::GetLastError;
    use winapi::um::fileapi::{ReadFile, WriteFile};
    use winapi::um::handleapi::{CloseHandle, DuplicateHandle};
    use winapi::um::namedpipeapi::{ConnectNamedPipe, CreateNamedPipeW};
    use winapi::um::ntsecapi::SystemFunction036;
    use winapi::um::processthreadsapi::GetCurrentProcess;
    use winapi::um::winnt::HANDLE;

    // Stable Win32 ABI values — defined locally so the build doesn't depend on
    // which winapi constant modules are feature-gated in.
    const PIPE_ACCESS_DUPLEX: DWORD = 0x0000_0003;
    const PIPE_TYPE_BYTE: DWORD = 0x0000_0000;
    const PIPE_READMODE_BYTE: DWORD = 0x0000_0000;
    const PIPE_WAIT: DWORD = 0x0000_0000;
    const PIPE_UNLIMITED_INSTANCES: DWORD = 255;
    const DUPLICATE_SAME_ACCESS: DWORD = 0x0000_0002;
    const ERROR_PIPE_CONNECTED: DWORD = 535;
    const ERROR_BROKEN_PIPE: DWORD = 109;
    const ERROR_PIPE_NOT_CONNECTED: DWORD = 233;
    const ERROR_NO_DATA: DWORD = 232;
    // 64 KiB per direction, matching the daemon's read scratch.
    const PIPE_BUFFER_SIZE: DWORD = 65536;

    /// `INVALID_HANDLE_VALUE` is `(HANDLE)-1`.
    fn invalid_handle() -> HANDLE {
        -1isize as HANDLE
    }

    /// NUL-terminated UTF-16, as `CreateNamedPipeW` expects. The daemon is handed
    /// the exact pipe path by the Node spawner (`\\?\pipe\orca-terminal-host-…`),
    /// so no derivation happens here — just widen the string verbatim.
    fn to_wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// One `CreateNamedPipeW` instance of the pipe name. A freshly created
    /// instance is connectable: a client's `CreateFile` can land on it before the
    /// server's `ConnectNamedPipe`, which then returns `ERROR_PIPE_CONNECTED`
    /// (success) — the property the pre-arm in `accept()` relies on.
    fn create_instance(wide_name: &[u16]) -> io::Result<HANDLE> {
        // SAFETY: wide_name is NUL-terminated; a NULL security-attributes
        // pointer takes the default (owner) descriptor.
        let handle = unsafe {
            CreateNamedPipeW(
                wide_name.as_ptr(),
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                PIPE_UNLIMITED_INSTANCES,
                PIPE_BUFFER_SIZE,
                PIPE_BUFFER_SIZE,
                0,
                ptr::null_mut(),
            )
        };
        if handle == invalid_handle() {
            return Err(io::Error::last_os_error());
        }
        Ok(handle)
    }

    /// Holds the pipe name plus the pre-created next instance (see the module
    /// doc's `ERROR_PIPE_BUSY` hardening).
    pub struct NamedPipeListener {
        wide_name: Vec<u16>,
        pending: Option<HANDLE>,
    }

    // A Win32 HANDLE is process-global and safe to move across threads; the
    // listener itself is only ever driven from the daemon's accept loop.
    unsafe impl Send for NamedPipeListener {}

    impl NamedPipeListener {
        /// Bind creates the first instance immediately so the pipe name exists
        /// (and health probes can connect) before the caller announces
        /// "listening" — and so an invalid name fails here, not on first accept.
        /// (No FILE_FLAG_FIRST_PIPE_INSTANCE: a same-user process holding the
        /// name would still succeed as an extra instance; the launcher's
        /// kill-stale-daemon step owns that exclusion, and the flag would race a
        /// dying predecessor's handle teardown into a spurious bind failure.)
        pub fn bind(name: &str) -> io::Result<Self> {
            let wide_name = to_wide(name);
            let first = create_instance(&wide_name)?;
            Ok(Self {
                wide_name,
                pending: Some(first),
            })
        }

        /// Block until a client connects to the pre-created instance, then
        /// pre-arm the next instance before returning the connected one.
        pub fn accept(&mut self) -> io::Result<NamedPipeStream> {
            loop {
                let handle = match self.pending.take() {
                    Some(pending) => pending,
                    // A previous best-effort pre-arm failed; retry it here where
                    // the error can propagate to the accept loop.
                    None => create_instance(&self.wide_name)?,
                };
                // SAFETY: `handle` is a valid listening instance; NULL overlapped =
                // blocking connect.
                let connected = unsafe { ConnectNamedPipe(handle, ptr::null_mut()) };
                if connected == FALSE {
                    let err = unsafe { GetLastError() };
                    // ERROR_NO_DATA: the client connected to the pre-armed
                    // instance and vanished before we listened — routine for
                    // liveness probes that connect and immediately destroy.
                    // Not an error; drop the instance and wait on a fresh one.
                    if err == ERROR_NO_DATA {
                        unsafe { CloseHandle(handle) };
                        continue;
                    }
                    // A client that connected between create and connect is success.
                    if err != ERROR_PIPE_CONNECTED {
                        unsafe { CloseHandle(handle) };
                        return Err(io::Error::from_raw_os_error(err as i32));
                    }
                }
                // Pre-arm the next instance NOW (not at the top of the next
                // accept): a client dialing while this connection is being handed
                // off finds a free instance instead of ERROR_PIPE_BUSY.
                // Best-effort — on failure the next accept() recreates and
                // surfaces the error.
                self.pending = create_instance(&self.wide_name).ok();
                return Ok(NamedPipeStream { handle });
            }
        }
    }

    impl Drop for NamedPipeListener {
        fn drop(&mut self) {
            if let Some(pending) = self.pending.take() {
                // SAFETY: `pending` is a handle this listener created and still owns.
                unsafe { CloseHandle(pending) };
            }
        }
    }

    /// One connected pipe instance. Owns its handle; `Drop` closes it (destroying
    /// the instance once every clone is dropped, which the peer sees as EOF).
    pub struct NamedPipeStream {
        handle: HANDLE,
    }

    // A Win32 HANDLE is process-global and safe to move/use across threads (the
    // daemon reads on one thread and writes on another via a clone).
    unsafe impl Send for NamedPipeStream {}

    impl NamedPipeStream {
        /// Duplicate the handle so a reader and a writer thread each own one,
        /// mirroring `UnixStream::try_clone`.
        pub fn try_clone(&self) -> io::Result<Self> {
            let mut target: HANDLE = invalid_handle();
            let ok = unsafe {
                DuplicateHandle(
                    GetCurrentProcess(),
                    self.handle,
                    GetCurrentProcess(),
                    &mut target,
                    0,
                    FALSE,
                    DUPLICATE_SAME_ACCESS,
                )
            };
            if ok == FALSE {
                return Err(io::Error::last_os_error());
            }
            Ok(NamedPipeStream { handle: target })
        }
    }

    impl Read for NamedPipeStream {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let mut read: DWORD = 0;
            let ok = unsafe {
                ReadFile(
                    self.handle,
                    buf.as_mut_ptr() as LPVOID,
                    buf.len() as DWORD,
                    &mut read,
                    ptr::null_mut(),
                )
            };
            if ok == FALSE {
                let err = unsafe { GetLastError() };
                // Peer closed -> EOF, matching UnixStream::read returning 0.
                if err == ERROR_BROKEN_PIPE
                    || err == ERROR_PIPE_NOT_CONNECTED
                    || err == ERROR_NO_DATA
                {
                    return Ok(0);
                }
                return Err(io::Error::from_raw_os_error(err as i32));
            }
            Ok(read as usize)
        }
    }

    impl Write for NamedPipeStream {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            let mut written: DWORD = 0;
            let ok = unsafe {
                WriteFile(
                    self.handle,
                    buf.as_ptr() as LPCVOID,
                    buf.len() as DWORD,
                    &mut written,
                    ptr::null_mut(),
                )
            };
            if ok == FALSE {
                return Err(io::Error::last_os_error());
            }
            Ok(written as usize)
        }

        fn flush(&mut self) -> io::Result<()> {
            // Deliberately NOT FlushFileBuffers: on a pipe it blocks until the peer
            // drains, which can deadlock the daemon. WriteFile already delivered the
            // bytes; the unix path likewise only flushes to the kernel buffer.
            Ok(())
        }
    }

    impl Drop for NamedPipeStream {
        fn drop(&mut self) {
            // Just close: DisconnectNamedPipe would forcibly tear down the shared
            // instance while a clone may still be using the other direction.
            unsafe { CloseHandle(self.handle) };
        }
    }

    /// Fill `buf` with OS entropy via RtlGenRandom (advapi32 `SystemFunction036`) —
    /// the daemon's Windows token source (the unix path uses `/dev/urandom`).
    pub fn fill_random(buf: &mut [u8]) -> io::Result<()> {
        let ok = unsafe { SystemFunction036(buf.as_mut_ptr() as *mut c_void, buf.len() as u32) };
        if ok == 0 {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "SystemFunction036 (RtlGenRandom) failed",
            ));
        }
        Ok(())
    }
}

#[cfg(windows)]
pub use imp::{fill_random, NamedPipeListener, NamedPipeStream};
