//! Persistence functions for IP deny list.

use super::types::PersistentState;
use ipnet::{Ipv4Net, Ipv6Net};
use roaring::RoaringBitmap;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter};
use std::path::Path;
use tracing::{debug, warn};

/// Save a snapshot of the deny list to disk.
pub(super) fn save_snapshot(snapshot: &super::IpDenyListSnapshot) -> Result<(), std::io::Error> {
    save(
        &snapshot.ipv4_bitmap,
        &snapshot.ipv4_cidrs,
        &snapshot.ipv6_cidrs,
        &snapshot.metadata,
        &snapshot.persist_path,
    )
}

/// Save the deny list to disk using MessagePack.
///
/// Uses atomic write (temp file + rename) to prevent corruption.
pub(super) fn save(
    ipv4_bitmap: &RoaringBitmap,
    ipv4_cidrs: &[Ipv4Net],
    ipv6_cidrs: &[Ipv6Net],
    metadata: &HashMap<String, super::types::BanMetadata>,
    persist_path: &Path,
) -> Result<(), std::io::Error> {
    let state = PersistentState {
        ipv4_singles: ipv4_bitmap.iter().collect(),
        ipv4_cidrs: ipv4_cidrs.iter().map(|n| n.to_string()).collect(),
        ipv6_cidrs: ipv6_cidrs.iter().map(|n| n.to_string()).collect(),
        metadata: metadata.clone(),
    };

    // Write to temp file first
    let temp_path = persist_path.with_extension("json.tmp");
    let file = File::create(&temp_path)?;
    let writer = BufWriter::new(file);

    serde_json::to_writer(writer, &state).map_err(|e| std::io::Error::other(e))?;

    // Atomic rename
    fs::rename(&temp_path, persist_path)?;

    debug!(path = %persist_path.display(), "IP deny list saved");
    Ok(())
}

/// Load deny list from JSON file.
#[allow(clippy::type_complexity)]
pub(super) fn load(
    path: &Path,
) -> Result<
    (
        RoaringBitmap,
        Vec<Ipv4Net>,
        Vec<Ipv6Net>,
        HashMap<String, super::types::BanMetadata>,
    ),
    Box<dyn std::error::Error>,
> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let state: PersistentState = serde_json::from_reader(reader)?;

    let mut ipv4_bitmap = RoaringBitmap::new();
    let mut ipv4_cidrs = Vec::new();
    let mut ipv6_cidrs = Vec::new();

    // Restore IPv4 singles
    for ip_u32 in state.ipv4_singles {
        ipv4_bitmap.insert(ip_u32);
    }

    // Restore IPv4 CIDRs
    for cidr_str in state.ipv4_cidrs {
        if let Ok(net) = cidr_str.parse::<Ipv4Net>() {
            ipv4_cidrs.push(net);
        } else {
            warn!(cidr = %cidr_str, "Failed to parse IPv4 CIDR from persistence");
        }
    }

    // Restore IPv6 CIDRs
    for cidr_str in state.ipv6_cidrs {
        if let Ok(net) = cidr_str.parse::<Ipv6Net>() {
            ipv6_cidrs.push(net);
        } else {
            warn!(cidr = %cidr_str, "Failed to parse IPv6 CIDR from persistence");
        }
    }

    Ok((ipv4_bitmap, ipv4_cidrs, ipv6_cidrs, state.metadata))
}
