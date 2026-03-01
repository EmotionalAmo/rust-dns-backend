use std::collections::HashMap;
use std::fs;

/// Parses the system ARP table to build a mapping from IP address to MAC address.
/// Specifically looks at /proc/net/arp on Linux systems.
pub fn get_arp_map() -> HashMap<String, String> {
    let mut map = HashMap::new();

    // Read the ARP table file
    if let Ok(contents) = fs::read_to_string("/proc/net/arp") {
        for line in contents.lines().skip(1) {
            let parts: Vec<&str> = line.split_whitespace().collect();
            // Format of /proc/net/arp:
            // IP address       HW type     Flags       HW address            Mask     Device
            // 192.168.100.1    0x1         0x2         aa:bb:cc:dd:ee:ff     *        eth0
            if parts.len() >= 4 {
                let ip = parts[0];
                let mac = parts[3];
                // Ignore incomplete ARP entries
                if mac != "00:00:00:00:00:00" && !mac.is_empty() {
                    map.insert(ip.to_string(), mac.to_string());
                }
            }
        }
    }

    map
}
