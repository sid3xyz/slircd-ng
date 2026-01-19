use crate::state::MemberModes;

/// WHOX field request parsed from %fields string.
#[derive(Default, Clone)]
pub struct WhoxFields {
    pub token: bool,                 // t
    pub channel: bool,               // c
    pub username: bool,              // u
    pub ip: bool,                    // i
    pub hostname: bool,              // h
    pub server: bool,                // s
    pub nick: bool,                  // n
    pub flags: bool,                 // f
    pub hopcount: bool,              // d
    pub idle: bool,                  // l
    pub account: bool,               // a
    pub oplevel: bool,               // o
    pub realname: bool,              // r
    pub query_token: Option<String>, // The token value if provided
}

impl WhoxFields {
    /// Parse WHOX fields from a string like "%cuhnar" or "%afnt,42"
    pub fn parse(s: &str) -> Option<Self> {
        if !s.starts_with('%') {
            return None;
        }
        let s = &s[1..]; // Remove %

        // Check for token: %fields,token
        let (fields_str, token_value) = if let Some(comma_pos) = s.find(',') {
            let fields = &s[..comma_pos];
            let token = &s[comma_pos + 1..];
            // Token must be 1-3 digits
            if token.len() > 3 || token.is_empty() || !token.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            (fields, Some(token.to_string()))
        } else {
            (s, None)
        };

        let mut result = WhoxFields {
            query_token: token_value,
            ..Default::default()
        };

        for c in fields_str.chars() {
            match c {
                't' => result.token = true,
                'c' => result.channel = true,
                'u' => result.username = true,
                'i' => result.ip = true,
                'h' => result.hostname = true,
                's' => result.server = true,
                'n' => result.nick = true,
                'f' => result.flags = true,
                'd' => result.hopcount = true,
                'l' => result.idle = true,
                'a' => result.account = true,
                'o' => result.oplevel = true,
                'r' => result.realname = true,
                _ => {} // Ignore unknown fields per spec
            }
        }

        // 't' requires a token value
        if result.token && result.query_token.is_none() {
            return None;
        }

        Some(result)
    }
}

/// User info needed for WHO/WHOX replies.
pub struct WhoUserInfo<'a> {
    pub nick: &'a str,
    pub user: &'a str,
    pub visible_host: &'a str,
    pub realname: &'a str,
    pub account: Option<&'a str>,
    pub is_away: bool,
    pub is_oper: bool,
    pub is_bot: bool,
    pub channel_prefixes: String,
}

/// Build prefix string for WHO flags based on member modes and multi-prefix setting.
pub fn get_member_prefixes(member_modes: &MemberModes, multi_prefix: bool) -> String {
    if multi_prefix {
        member_modes.all_prefix_chars()
    } else if let Some(prefix) = member_modes.prefix_char() {
        prefix.to_string()
    } else {
        String::new()
    }
}

/// Simple wildcard matching for WHO masks.
/// Supports * (match any) and ? (match single char).
///
/// This implementation is iterative to avoid stack overflow and excessive allocations.
#[must_use]
pub fn matches_mask(value: &str, mask: &str) -> bool {
    if mask == "*" {
        return true;
    }
    if !mask.contains('*') && !mask.contains('?') {
        return value == mask;
    }

    let v_chars: Vec<char> = value.chars().collect();
    let m_chars: Vec<char> = mask.chars().collect();

    let mut v_idx = 0;
    let mut m_idx = 0;

    let mut star_m_idx = None;
    let mut match_v_idx = 0;

    while v_idx < v_chars.len() {
        if m_idx < m_chars.len() && (m_chars[m_idx] == '?' || m_chars[m_idx] == v_chars[v_idx]) {
            // Case 1: Exact match or '?'
            v_idx += 1;
            m_idx += 1;
        } else if m_idx < m_chars.len() && m_chars[m_idx] == '*' {
            // Case 2: '*' found, record position and advance mask
            star_m_idx = Some(m_idx);
            m_idx += 1;
            match_v_idx = v_idx;
        } else if let Some(star_idx) = star_m_idx {
            // Case 3: Mismatch, but we have a previous '*', backtrack
            // Try matching '*' against one more character of value
            m_idx = star_idx + 1;
            match_v_idx += 1;
            v_idx = match_v_idx;
        } else {
            // Case 4: Mismatch and no '*' to backtrack to
            return false;
        }
    }

    // Check for trailing '*' in mask
    while m_idx < m_chars.len() && m_chars[m_idx] == '*' {
        m_idx += 1;
    }

    m_idx == m_chars.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_matches_mask() {
        assert!(matches_mask("test", "test"));
        assert!(matches_mask("test", "*"));
        assert!(matches_mask("test", "t*"));
        assert!(matches_mask("test", "*t"));
        assert!(matches_mask("test", "t*t"));
        assert!(matches_mask("test", "te?t"));
        assert!(matches_mask("test", "????"));
        assert!(!matches_mask("test", "?????"));
        assert!(!matches_mask("test", "best"));
        assert!(matches_mask("testing", "test*"));
        assert!(matches_mask("testing", "*ing"));
        assert!(matches_mask("foo", "*?*"));
        assert!(matches_mask("f", "*?*"));
        assert!(matches_mask("", "*"));
        assert!(!matches_mask("", "?"));
    }

    #[test]
    fn test_matches_mask_unicode() {
        // Test unicode behavior
        assert!(matches_mask("🔥", "?")); // Should work with chars, fail with bytes
        assert!(matches_mask("🔥", "*"));
    }

    #[test]
    fn test_whox_fields_parse() {
        // Basic field parsing - no token
        let fields = WhoxFields::parse("%r").unwrap();
        assert!(fields.realname);
        assert!(!fields.nick);
        assert!(fields.query_token.is_none());

        // Multiple fields
        let fields = WhoxFields::parse("%cunhar").unwrap();
        assert!(fields.channel);
        assert!(fields.username);
        assert!(fields.nick);
        assert!(fields.hostname);
        assert!(fields.account);
        assert!(fields.realname);

        // With token
        let fields = WhoxFields::parse("%t,42").unwrap();
        assert!(fields.token);
        assert_eq!(fields.query_token, Some("42".to_string()));

        // Token in middle of flags
        let fields = WhoxFields::parse("%cnt,123").unwrap();
        assert!(fields.channel);
        assert!(fields.nick);
        assert!(fields.token);
        assert_eq!(fields.query_token, Some("123".to_string()));

        // 't' without token value should fail
        assert!(WhoxFields::parse("%t").is_none());
        assert!(WhoxFields::parse("%cnt").is_none());

        // Not starting with % should fail
        assert!(WhoxFields::parse("r").is_none());
        assert!(WhoxFields::parse("cunhar").is_none());
    }

    #[test]
    fn test_get_member_prefixes_op_single() {
        let mut modes = MemberModes::default();
        modes.op = true;
        assert_eq!(get_member_prefixes(&modes, false), "@");
    }

    #[test]
    fn test_get_member_prefixes_op_multi() {
        let mut modes = MemberModes::default();
        modes.op = true;
        assert_eq!(get_member_prefixes(&modes, true), "@");
    }

    #[test]
    fn test_get_member_prefixes_voice_single() {
        let mut modes = MemberModes::default();
        modes.voice = true;
        assert_eq!(get_member_prefixes(&modes, false), "+");
    }

    #[test]
    fn test_get_member_prefixes_op_voice_single() {
        let mut modes = MemberModes::default();
        modes.op = true;
        modes.voice = true;
        // Should return highest rank only
        assert_eq!(get_member_prefixes(&modes, false), "@");
    }

    #[test]
    fn test_get_member_prefixes_op_voice_multi() {
        let mut modes = MemberModes::default();
        modes.op = true;
        modes.voice = true;
        // Should return all prefixes
        assert_eq!(get_member_prefixes(&modes, true), "@+");
    }

    #[test]
    fn test_get_member_prefixes_none() {
        let modes = MemberModes::default();
        assert_eq!(get_member_prefixes(&modes, false), "");
    }

    #[test]
    fn test_matches_mask_dos() {
        // Create a pathological mask pattern: *a*a*a...
        let mut mask = String::new();
        for _ in 0..200 {
            mask.push_str("*a");
        }
        mask.push('*');

        // Create a matching string
        let mut value = String::new();
        for _ in 0..200 {
            value.push_str("ba");
        }

        assert!(matches_mask(&value, &mask));
    }

    #[test]
    fn test_matches_mask_backtracking() {
        // Test backtracking cases that might be tricky
        assert!(matches_mask("ab", "*b"));
        assert!(matches_mask("aaab", "a*b"));
        assert!(matches_mask("mississippi", "m*issi*ippi"));
        // This case forces backtracking: * matches "aa", then mismatch at b, so * consumes "aaa"
        assert!(matches_mask("aaab", "*b"));
    }
}
