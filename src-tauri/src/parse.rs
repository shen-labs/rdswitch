//! 把使用者貼上的文字解析成伺服器設定。支援：
//! 1. RustDesk 匯出的設定字串（base64 的 JSON，字串可能被反轉，可有 `config=` 前綴）
//! 2. 純 JSON：`{"host":..., "relay":..., "api":..., "key":...}`
//! 3. 帶標籤的文字：「ID 伺服器: x」「Relay: x」「API: x」「Key: x」
//! 4. 單獨的 `host[:port]`，之後可接一個 key

use crate::i18n::tr;
use crate::profiles::Profile;
use base64::engine::general_purpose::{STANDARD_NO_PAD, URL_SAFE_NO_PAD};
use base64::Engine;
use regex::Regex;
use std::sync::OnceLock;

fn re(cell: &'static OnceLock<Regex>, pat: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pat).unwrap())
}

fn from_json(raw: &[u8]) -> Option<Profile> {
    let v: serde_json::Value = serde_json::from_slice(raw).ok()?;
    let obj = v.as_object()?;
    let pick = |keys: &[&str]| {
        keys.iter()
            .find_map(|k| obj.get(*k).and_then(|x| x.as_str()).map(str::trim).filter(|s| !s.is_empty()))
            .unwrap_or("")
            .to_string()
    };
    let p = Profile {
        name: String::new(),
        server: pick(&["host", "server", "custom-rendezvous-server", "rendezvous_server"]),
        relay: pick(&["relay", "relay-server"]),
        api: pick(&["api", "api-server"]),
        key: pick(&["key"]),
    };
    (!p.server.is_empty() || !p.key.is_empty()).then_some(p)
}

fn b64_decode(s: &str) -> Option<Vec<u8>> {
    let s: String = s.split_whitespace().collect();
    let s = s.trim_end_matches('=');
    URL_SAFE_NO_PAD.decode(s).or_else(|_| STANDARD_NO_PAD.decode(s)).ok()
}

fn try_config_string(text: &str) -> Option<Profile> {
    static CHARS: OnceLock<Regex> = OnceLock::new();
    let mut s = text.trim();
    if s.get(..7).is_some_and(|p| p.eq_ignore_ascii_case("config=")) {
        s = &s[7..];
    }
    if s.len() < 16 || !re(&CHARS, r"^[A-Za-z0-9+/=_\-\s]+$").is_match(s) {
        return None;
    }
    let reversed: String = s.chars().rev().collect(); // RustDesk 會把字串反轉
    [reversed.as_str(), s].iter().find_map(|cand| b64_decode(cand).and_then(|raw| from_json(&raw)))
}

fn try_labelled(text: &str) -> Option<Profile> {
    static LINE: OnceLock<Regex> = OnceLock::new();
    static SERVER: OnceLock<Regex> = OnceLock::new();
    static RELAY: OnceLock<Regex> = OnceLock::new();
    static API: OnceLock<Regex> = OnceLock::new();
    static KEY: OnceLock<Regex> = OnceLock::new();
    let line_re = re(&LINE, r"^\s*([^:=：]+?)\s*[:=：]\s*(.+?)\s*$");
    let server_re = re(&SERVER, r"(?i)^(?:id\s*(?:server|伺服器|服務器|服务器|サーバー)|rendezvous|host|server|主機|主机|伺服器|服务器|サーバー)$");
    let relay_re = re(&RELAY, r"(?i)relay|中繼|中继|リレー");
    let api_re = re(&API, r"(?i)api");
    let key_re = re(&KEY, r"(?i)^(?:key|公鑰|公钥|公開鍵|public\s*key)$");

    let mut p = Profile::default();
    for line in text.lines() {
        let Some(m) = line_re.captures(line) else { continue };
        let label = &m[1];
        let value = m[2].trim_matches(|c| c == '\'' || c == '"').to_string();
        // relay / api 先比對，避免寬鬆的 server 標籤吃掉 "relay server"
        let slot = if relay_re.is_match(label) {
            &mut p.relay
        } else if api_re.is_match(label) {
            &mut p.api
        } else if key_re.is_match(label) {
            &mut p.key
        } else if server_re.is_match(label) {
            &mut p.server
        } else {
            continue;
        };
        if slot.is_empty() {
            *slot = value;
        }
    }
    (!p.server.is_empty() || !p.key.is_empty()).then_some(p)
}

fn try_bare(text: &str) -> Option<Profile> {
    static BARE: OnceLock<Regex> = OnceLock::new();
    let t: Vec<&str> = text.split_whitespace().collect();
    if t.is_empty() || t.len() > 2 || !re(&BARE, r"^[\w.\-]+(:\d+)?$").is_match(t[0]) {
        return None;
    }
    Some(Profile { server: t[0].into(), key: t.get(1).map(|s| s.to_string()).unwrap_or_default(), ..Default::default() })
}

pub fn parse_server_text(text: &str) -> Result<Profile, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err(tr("err.empty_input"));
    }
    from_json(text.as_bytes())
        .or_else(|| try_config_string(text))
        .or_else(|| try_labelled(text))
        .or_else(|| try_bare(text))
        .ok_or_else(|| tr("err.unrecognized"))
}

/// 由伺服器位址產生預設的 profile 名稱。
pub fn default_name(server: &str) -> String {
    let host = server.split(':').next().unwrap_or("");
    let n: String = host.chars().map(|c| if c.is_alphanumeric() || "._-".contains(c) { c } else { '_' }).collect();
    if n.is_empty() { "imported".into() } else { n }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_string_variants() {
        let json = r#"{"host":"203.0.113.1:21116","relay":"203.0.113.1:21117","key":"AbC+/123="}"#;
        let b = URL_SAFE_NO_PAD.encode(json);
        let rev: String = b.chars().rev().collect();
        for s in [rev.clone(), format!("config={rev}"), b, json.to_string()] {
            let p = parse_server_text(&s).unwrap_or_else(|e| panic!("{s}: {e}"));
            assert_eq!(p.server, "203.0.113.1:21116");
            assert_eq!(p.key, "AbC+/123=");
        }
    }

    #[test]
    fn labelled() {
        let p = parse_server_text("ID 伺服器: a.example.com\nRelay 伺服器: b.example.com\nAPI: http://x\nKey: K1=").unwrap();
        assert_eq!((p.server.as_str(), p.relay.as_str(), p.api.as_str(), p.key.as_str()),
                   ("a.example.com", "b.example.com", "http://x", "K1="));
    }

    #[test]
    fn bare_and_invalid() {
        assert_eq!(parse_server_text("host.example.com KEY=").unwrap().key, "KEY=");
        assert!(parse_server_text("???").is_err());
        assert!(parse_server_text("  ").is_err());
    }

    #[test]
    fn names() {
        assert_eq!(default_name("203.0.113.1:21116"), "203.0.113.1");
        assert_eq!(default_name(""), "imported");
    }
}
