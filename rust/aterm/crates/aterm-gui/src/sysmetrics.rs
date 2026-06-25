// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! Dependency-free OS metric probes for the HUD framework (CPU load, memory,
//! network byte counters), behind safe `Option`-returning wrappers. ALL raw `libc`
//! FFI lives here (the "one seam for unsafe" discipline, like aterm-pty). macOS is
//! the implemented target; off-macOS every probe returns `None` so the panels paint
//! "n/a" and never break the build.
//!
//! Honesty: these are WHOLE-MACHINE figures. macOS exposes no public per-process
//! network counter (only the private NetworkStatistics framework), so per-app
//! traffic is reported by the process itself via the app-fed `metric` channel, not
//! here.

/// 1-minute system load average, or `None` if unavailable.
#[must_use]
pub(crate) fn load_avg_1m() -> Option<f64> {
    #[cfg(target_os = "macos")]
    {
        let mut la = [0.0f64; 3];
        // SAFETY: `getloadavg` fills up to `nelem` doubles into the buffer; we pass a
        // valid 3-element array and request 3. Returns the count written, or -1.
        let n = unsafe { libc::getloadavg(la.as_mut_ptr(), 3) };
        if n >= 1 { Some(la[0]) } else { None }
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

/// Logical CPU count (for normalizing the load average), default 1.
#[must_use]
pub(crate) fn ncpu() -> u32 {
    sysctl_u64("hw.logicalcpu")
        .or_else(|| sysctl_u64("hw.ncpu"))
        .unwrap_or(1) as u32
}

/// Total physical RAM in bytes, or `None`.
#[must_use]
pub(crate) fn mem_total() -> Option<u64> {
    sysctl_u64("hw.memsize")
}

/// Fraction (0..1) of RAM in active use (active + wired + compressed), a proxy for
/// memory pressure; `None` if unavailable.
#[must_use]
pub(crate) fn mem_used_frac() -> Option<f64> {
    #[cfg(target_os = "macos")]
    {
        let total = mem_total()? as f64;
        let page = sysctl_u64("hw.pagesize").unwrap_or(4096) as f64;
        let vm = vm_stats64()?;
        let used = (vm.active_count as f64
            + vm.wire_count as f64
            + u64::from(vm.compressor_page_count) as f64)
            * page;
        if total > 0.0 {
            Some((used / total).clamp(0.0, 1.0))
        } else {
            None
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

/// Cumulative whole-machine network (rx, tx) byte counters across non-loopback
/// links, or `None`. Counters are 32-bit on macOS `if_data`; callers wrapping-sub.
#[must_use]
pub(crate) fn net_bytes() -> Option<(u64, u64)> {
    #[cfg(target_os = "macos")]
    {
        net_bytes_macos()
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

// --- macOS implementations --------------------------------------------------

#[cfg(target_os = "macos")]
fn sysctl_u64(name: &str) -> Option<u64> {
    use std::ffi::CString;
    let cname = CString::new(name).ok()?;
    let mut val: u64 = 0;
    let mut len = std::mem::size_of::<u64>();
    // SAFETY: `sysctlbyname` writes up to `len` bytes into `val`; we pass a valid u64
    // out-param and its size. hw.* keys return a 64- or 32-bit integer.
    let rc = unsafe {
        libc::sysctlbyname(
            cname.as_ptr(),
            std::ptr::addr_of_mut!(val).cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 {
        return None;
    }
    // Some keys (hw.ncpu, hw.pagesize) are 32-bit; mask if only 4 bytes were written.
    if len == 4 {
        Some(u64::from(val as u32))
    } else {
        Some(val)
    }
}

#[cfg(not(target_os = "macos"))]
#[allow(dead_code)]
fn sysctl_u64(_name: &str) -> Option<u64> {
    None
}

// `mach_port_deallocate` is not re-exported by `libc`; declare it. It lives in
// libSystem (always linked on macOS). Used to release the send-right reference that
// `mach_host_self()` adds to this task's IPC space on every call.
#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn mach_port_deallocate(
        task: libc::mach_port_t,
        name: libc::mach_port_t,
    ) -> libc::kern_return_t;
}

#[cfg(target_os = "macos")]
fn vm_stats64() -> Option<libc::vm_statistics64> {
    // SAFETY: zeroed POD; host_statistics64 fills it. mach_host_self() returns the
    // host port (the deprecation is cosmetic; the data fn is not deprecated).
    let mut stats: libc::vm_statistics64 = unsafe { std::mem::zeroed() };
    let mut count = (std::mem::size_of::<libc::vm_statistics64>()
        / std::mem::size_of::<libc::integer_t>())
        as libc::mach_msg_type_number_t;
    // SAFETY: `mach_host_self()` returns a send right to the host name port AND adds a
    // user reference to it in our IPC space on EVERY call — so it must be paired with
    // `mach_port_deallocate` below, or the reference count climbs ~3×/s (the HUD poll
    // rate) for the process lifetime. The deprecation on the symbol is cosmetic.
    #[allow(deprecated)]
    let host = unsafe { libc::mach_host_self() };
    // SAFETY: valid host port, HOST_VM_INFO64 flavor, out-buffer + its element count.
    let rc = unsafe {
        libc::host_statistics64(
            host,
            libc::HOST_VM_INFO64,
            std::ptr::addr_of_mut!(stats).cast(),
            &mut count,
        )
    };
    // SAFETY: release the send-right reference added by `mach_host_self()` above.
    // `mach_task_self_` is this task's own port (a `static` set up by libSystem); we
    // only read its value. Done on BOTH success and failure paths — the reference is
    // added regardless of `host_statistics64`'s result. Ignoring the return is fine:
    // a failed deallocate can't make the leak worse than not calling it. The
    // deprecation (libc suggests the `mach2` crate) is cosmetic — we keep the existing
    // dependency-free `libc`-only seam, matching `mach_host_self()` above.
    #[allow(deprecated)]
    unsafe {
        let _ = mach_port_deallocate(libc::mach_task_self_, host);
    }
    if rc == libc::KERN_SUCCESS {
        Some(stats)
    } else {
        None
    }
}

#[cfg(target_os = "macos")]
fn net_bytes_macos() -> Option<(u64, u64)> {
    let mut ifap: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: getifaddrs allocates a linked list into `ifap`; freed below.
    if unsafe { libc::getifaddrs(&mut ifap) } != 0 || ifap.is_null() {
        return None;
    }
    let (mut rx, mut tx) = (0u64, 0u64);
    let mut cur = ifap;
    // SAFETY: walk the NUL-terminated `ifa_next` list; each node is valid until
    // freeifaddrs. AF_LINK nodes carry an `if_data` in `ifa_data`.
    unsafe {
        while !cur.is_null() {
            let ifa = &*cur;
            if !ifa.ifa_addr.is_null()
                && i32::from((*ifa.ifa_addr).sa_family) == libc::AF_LINK
                && (ifa.ifa_flags & libc::IFF_LOOPBACK as u32) == 0
                && !ifa.ifa_data.is_null()
            {
                let d = &*(ifa.ifa_data as *const libc::if_data);
                rx += u64::from(d.ifi_ibytes);
                tx += u64::from(d.ifi_obytes);
            }
            cur = ifa.ifa_next;
        }
        libc::freeifaddrs(ifap);
    }
    Some((rx, tx))
}
