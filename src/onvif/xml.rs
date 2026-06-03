//! Tiny namespace-agnostic XML scanners for ONVIF SOAP responses. Ported
//! verbatim from the cornea agent — no XML crate, just byte scanning so it
//! tolerates whatever namespace prefixes a given camera uses.

/// Local name of a tag, stripping any `ns:` prefix.
pub fn local_name(tag: &str) -> &str {
    tag.find(':').map_or(tag, |i| &tag[i + 1..])
}

/// Position just after the `>` of the first opening tag whose local name is
/// `target`, scanning from `xml[from..]`.
pub fn scan_open_tag(xml: &str, target: &str, from: usize) -> Option<usize> {
    let mut i = from;
    while i < xml.len() {
        let lt = xml[i..].find('<')? + i;
        let rest = &xml[lt + 1..];
        if rest.starts_with('/') || rest.starts_with('!') || rest.starts_with('?') {
            i = lt + 1;
            continue;
        }
        let name_end = rest
            .find(|c: char| c == '>' || c == ' ' || c == '/')
            .unwrap_or(rest.len());
        if local_name(&rest[..name_end]) == target {
            let gt = xml[lt..].find('>')? + lt + 1;
            return Some(gt);
        }
        i = lt + 1;
    }
    None
}

/// Start position of `</…:target>` scanning from `xml[from..]`.
pub fn scan_close_tag(xml: &str, target: &str, from: usize) -> Option<usize> {
    let mut i = from;
    while i < xml.len() {
        let lt = xml[i..].find('<')? + i;
        let rest = &xml[lt + 1..];
        if !rest.starts_with('/') {
            i = lt + 1;
            continue;
        }
        let name_content = &rest[1..];
        let name_end = name_content.find('>').unwrap_or(name_content.len());
        if local_name(name_content[..name_end].trim()) == target {
            return Some(lt);
        }
        i = lt + 1;
    }
    None
}

/// Text content of the first tag (any namespace) whose local name is `target`.
pub fn extract_tag<'a>(xml: &'a str, target: &str) -> Option<&'a str> {
    let content_start = scan_open_tag(xml, target, 0)?;
    let end = scan_close_tag(xml, target, content_start)?;
    Some(xml[content_start..end].trim())
}

// ---- ONVIF response parsers -------------------------------------------------

pub fn parse_media_url(xml: &str, fallback: &str) -> String {
    extract_tag(xml, "Media")
        .and_then(|block| extract_tag(block, "XAddr"))
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| fallback.to_string())
}

/// `<…:Profiles token="Profile_1" …>` — the token is an attribute.
pub fn parse_first_profile_token(xml: &str) -> Option<String> {
    let mut i = 0;
    while i < xml.len() {
        let lt = xml[i..].find('<')? + i;
        let rest = &xml[lt + 1..];
        if rest.starts_with('/') || rest.starts_with('!') || rest.starts_with('?') {
            i = lt + 1;
            continue;
        }
        let name_end = rest.find(|c: char| c == '>' || c == ' ').unwrap_or(rest.len());
        if local_name(&rest[..name_end]) == "Profiles" {
            let tag_end = xml[lt..].find('>')? + lt;
            let attrs = &xml[lt..tag_end];
            if let Some(tok_pos) = attrs.find("token=\"") {
                let val_start = tok_pos + 7;
                if let Some(end_quote) = attrs[val_start..].find('"') {
                    return Some(attrs[val_start..val_start + end_quote].to_string());
                }
            }
        }
        i = lt + 1;
    }
    None
}

pub fn parse_uri(xml: &str) -> Option<String> {
    extract_tag(xml, "Uri").map(|s| s.to_string())
}

pub fn parse_xaddrs_and_scopes(xml: &str) -> Option<(String, Vec<String>)> {
    let xaddrs_text = extract_tag(xml, "XAddrs")?;
    let first_url = xaddrs_text.split_whitespace().next()?.to_string();
    let scopes: Vec<String> = extract_tag(xml, "Scopes")
        .unwrap_or("")
        .split_whitespace()
        .map(|s| s.to_string())
        .collect();
    Some((first_url, scopes))
}
