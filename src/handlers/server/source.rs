use slirc_proto::MessageRef;
use slirc_proto::sync::clock::ServerId;

/// Extract the source server SID from a server-to-server message prefix.
///
/// In TS6-style messages, the prefix is typically a server identifier (often a SID),
/// but some implementations encode it as `SID.server.name`.
pub fn extract_source_sid(msg: &MessageRef<'_>) -> Option<ServerId> {
    let prefix = msg.prefix.as_ref()?;

    let sid = if prefix.is_server() {
        prefix.raw.split('.').next()?
    } else {
        // For server messages, raw prefix is often just the SID (e.g., "00A")
        prefix.raw
    };

    Some(ServerId::new(sid.to_string()))
}
