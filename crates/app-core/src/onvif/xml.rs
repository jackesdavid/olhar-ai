//! Tiny namespace-agnostic XML scanners for ONVIF SOAP responses. Copied
//! verbatim from the ratatui `src/onvif/xml.rs` — no XML crate, just byte
//! scanning so it tolerates whatever namespace prefixes a given camera uses.

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
        let name_end = rest
            .find(|c: char| c == '>' || c == ' ')
            .unwrap_or(rest.len());
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

/// One media profile from a `GetProfiles` response: its `token` (needed for
/// `GetStreamUri`) plus the encoder resolution (0×0 if the profile has none), used to
/// rank profiles main (highest) vs sub (lowest).
#[derive(Debug, Clone)]
pub struct ProfileInfo {
    pub token: String,
    pub width: u32,
    pub height: u32,
}

impl ProfileInfo {
    /// Pixel area — the ranking key for picking the main (largest) / sub (smallest)
    /// profile. Unknown resolutions sort as 0.
    pub fn area(&self) -> u64 {
        self.width as u64 * self.height as u64
    }
}

/// Enumerate **all** media profiles (token + resolution) from a `GetProfiles` response.
/// Profiles are siblings (not nested), so each runs from its `<…:Profiles token="…">`
/// open tag to the next `</…:Profiles>`. The resolution is read from the profile's
/// `VideoEncoderConfiguration/Resolution` (`Width`/`Height`).
pub fn parse_profiles(xml: &str) -> Vec<ProfileInfo> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < xml.len() {
        let Some(lt) = xml[i..].find('<').map(|p| p + i) else {
            break;
        };
        let rest = &xml[lt + 1..];
        if rest.starts_with('/') || rest.starts_with('!') || rest.starts_with('?') {
            i = lt + 1;
            continue;
        }
        let name_end = rest
            .find(|c: char| c == '>' || c == ' ' || c == '/')
            .unwrap_or(rest.len());
        if local_name(&rest[..name_end]) != "Profiles" {
            i = lt + 1;
            continue;
        }
        let Some(tag_end) = xml[lt..].find('>').map(|p| p + lt) else {
            break;
        };
        let attrs = &xml[lt..tag_end];
        let token = attrs.find("token=\"").and_then(|p| {
            let s = p + 7;
            attrs[s..].find('"').map(|e| attrs[s..s + e].to_string())
        });
        let content_start = tag_end + 1;
        let close = scan_close_tag(xml, "Profiles", content_start).unwrap_or(xml.len());
        let block = &xml[content_start..close];
        let (width, height) = parse_resolution(block);
        if let Some(token) = token {
            out.push(ProfileInfo {
                token,
                width,
                height,
            });
        }
        i = close;
    }
    out
}

/// `<…:Resolution><…:Width>1920</…:Width><…:Height>1080</…:Height></…:Resolution>`
/// → `(1920, 1080)`; `(0, 0)` when absent or unparseable.
fn parse_resolution(block: &str) -> (u32, u32) {
    let Some(res) = extract_tag(block, "Resolution") else {
        return (0, 0);
    };
    let w = extract_tag(res, "Width")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let h = extract_tag(res, "Height")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    (w, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    // A trimmed two-profile GetProfiles response (main 1080p + sub 640×360), with the
    // typical `trt:`/`tt:` namespace prefixes a real camera uses.
    const PROFILES: &str = r#"<trt:GetProfilesResponse>
      <trt:Profiles token="MainProfile">
        <tt:VideoEncoderConfiguration>
          <tt:Resolution><tt:Width>1920</tt:Width><tt:Height>1080</tt:Height></tt:Resolution>
        </tt:VideoEncoderConfiguration>
      </trt:Profiles>
      <trt:Profiles token="SubProfile">
        <tt:VideoEncoderConfiguration>
          <tt:Resolution><tt:Width>640</tt:Width><tt:Height>360</tt:Height></tt:Resolution>
        </tt:VideoEncoderConfiguration>
      </trt:Profiles>
    </trt:GetProfilesResponse>"#;

    #[test]
    fn parse_profiles_enumerates_tokens_and_resolutions() {
        let p = parse_profiles(PROFILES);
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].token, "MainProfile");
        assert_eq!((p[0].width, p[0].height), (1920, 1080));
        assert_eq!(p[1].token, "SubProfile");
        assert_eq!((p[1].width, p[1].height), (640, 360));

        // main = largest area, sub = smallest area
        let main = p.iter().max_by_key(|x| x.area()).unwrap();
        let sub = p.iter().min_by_key(|x| x.area()).unwrap();
        assert_eq!(main.token, "MainProfile");
        assert_eq!(sub.token, "SubProfile");
    }

    #[test]
    fn parse_profiles_single_profile() {
        let xml = r#"<trt:Profiles token="Only">
          <tt:VideoEncoderConfiguration>
            <tt:Resolution><tt:Width>1280</tt:Width><tt:Height>720</tt:Height></tt:Resolution>
          </tt:VideoEncoderConfiguration>
        </trt:Profiles>"#;
        let p = parse_profiles(xml);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].token, "Only");
        // main == sub when there's only one profile (caller emits no distinct substream).
        let main = p.iter().max_by_key(|x| x.area()).unwrap();
        let sub = p.iter().min_by_key(|x| x.area()).unwrap();
        assert_eq!(main.token, sub.token);
    }
}

/// The stable device id from a ProbeMatch — the WS-Addressing
/// `EndpointReference/Address` (typically `urn:uuid:…`), which survives DHCP/IP
/// changes. Reads *inside* the EndpointReference element so it never picks up the
/// SOAP header's MessageID / RelatesTo (also UUIDs).
pub fn parse_endpoint_address(xml: &str) -> Option<String> {
    let er = extract_tag(xml, "EndpointReference")?;
    let addr = extract_tag(er, "Address")?;
    (!addr.is_empty()).then(|| addr.to_string())
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
