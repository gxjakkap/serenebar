//! Network speed: bytes in and out per second across the network adapters.

/// A speed in bytes per second, short enough for the tile: "840 KB/s", "2.4 MB/s", "31 MB/s".
pub fn fmt_rate(bytes_per_sec: f64) -> String {
    let kb = bytes_per_sec.max(0.0) / 1000.0;
    if kb < 1000.0 {
        format!("{kb:.0} KB/s")
    } else if kb < 10_000.0 {
        format!("{:.1} MB/s", kb / 1000.0)
    } else if kb < 1_000_000.0 {
        format!("{:.0} MB/s", kb / 1000.0)
    } else {
        format!("{:.1} GB/s", kb / 1_000_000.0)
    }
}

/// Bytes per second between two readings of a counter `secs` apart. A counter that went
/// backwards (an adapter went away) reads as no traffic.
pub fn rate(before: u64, after: u64, secs: f64) -> f64 {
    if secs <= 0.0 {
        return 0.0;
    }
    after.saturating_sub(before) as f64 / secs
}

#[cfg(windows)]
pub use win::*;

#[cfg(windows)]
mod win {
    use windows::Win32::NetworkManagement::IpHelper::*;
    use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;

    /// Bytes received and sent so far by the adapters that are up. Only real hardware counts:
    /// virtual switches, VPN tunnels and filter layers carry the same traffic again.
    pub fn counters() -> Option<(u64, u64)> {
        let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
        unsafe {
            GetIfTable2(&mut table).ok().ok()?;
            let t = &*table;
            let rows = std::slice::from_raw_parts(t.Table.as_ptr(), t.NumEntries as usize);
            let (mut rx, mut tx) = (0, 0);
            for r in rows {
                let flags = r.InterfaceAndOperStatusFlags._bitfield;
                let (hardware, filter) = (flags & 1 != 0, flags & 2 != 0);
                if hardware && !filter && r.OperStatus == IfOperStatusUp {
                    rx += r.InOctets;
                    tx += r.OutOctets;
                }
            }
            FreeMibTable(table as _);
            Some((rx, tx))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speeds() {
        assert_eq!(fmt_rate(0.0), "0 KB/s");
        assert_eq!(fmt_rate(840_400.0), "840 KB/s");
        assert_eq!(fmt_rate(2_440_000.0), "2.4 MB/s");
        assert_eq!(fmt_rate(31_200_000.0), "31 MB/s");
        assert_eq!(fmt_rate(1_250_000_000.0), "1.2 GB/s");
        assert_eq!(rate(1000, 3000, 2.0), 1000.0);
        assert_eq!(rate(5000, 10, 1.0), 0.0);
        assert_eq!(rate(0, 10, 0.0), 0.0);
    }
}
