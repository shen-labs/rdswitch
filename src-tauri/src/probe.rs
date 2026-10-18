//! 伺服器連線狀態：對 ID 伺服器（hbbs）的 TCP 與 UDP 21116 埠各做一次檢查並量測時間。
//!
//! - TCP：連線建立成功即算在線。
//! - UDP：沒有連線概念，必須對方「回應」才算數。這裡送出一個最小的 RustDesk 協定請求
//!   （`RegisterPeer`，使用假 ID），hbbs 會回 `RegisterPeerResponse`；伺服器不會因此儲存這個假 ID。
//!   回應內容會驗證，避免把非 RustDesk 的回應當成正常。
//!
//! 限制：只涵蓋 ID 伺服器；中繼（21117）與 API 埠沒有檢查。

use serde::Serialize;
use std::io::ErrorKind;
use std::net::{SocketAddr, TcpStream, ToSocketAddrs, UdpSocket};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// 錯誤代碼：由前端依使用者語言翻譯（後端不放任何顯示用文字）。
pub const CODE_TIMEOUT: &str = "timeout";
pub const CODE_REFUSED: &str = "refused";
pub const CODE_RESOLVE_FAILED: &str = "resolve_failed";
pub const CODE_NO_REPLY: &str = "no_reply";
pub const CODE_NOT_RUSTDESK: &str = "not_rustdesk";
pub const CODE_FAILED: &str = "failed";
pub const CODE_INTERNAL: &str = "internal";

/// RustDesk ID/Rendezvous 伺服器的預設埠（TCP 與 UDP 相同）。
pub const DEFAULT_PORT: u16 = 21116;

/// `RendezvousMessage { register_peer(6) = RegisterPeer { id(1) = "rdswitch-probe" } }`
/// 欄位編號取自 RustDesk 的 rendezvous.proto，並已用真實伺服器實測驗證。
const UDP_PROBE_ID: &[u8] = b"rdswitch-probe";
/// 回應的第一個位元組（protobuf tag）：register_peer_response(7) 或 configure_update(14)。
const UDP_REPLY_TAGS: [u8; 2] = [0x3a, 0x72];

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Reach {
    pub server: String,
    /// TCP 是否連得上
    pub ok: bool,
    pub ms: Option<u64>,
    pub error: Option<String>,
    /// UDP 是否收到合法的 RustDesk 回應
    pub udp_ok: bool,
    pub udp_ms: Option<u64>,
    pub udp_error: Option<String>,
}

/// 拆出主機與埠；沒寫埠時用預設埠。支援 `host`、`host:port`、`[ipv6]:port`。
pub fn split_host_port(server: &str) -> (String, u16) {
    let s = server.trim();
    if let Some((h, p)) = s.rsplit_once(':') {
        let is_port = !p.is_empty() && p.chars().all(|c| c.is_ascii_digit());
        // 裸 IPv6（含多個冒號、沒有方括號）無法判斷埠，視為沒寫埠
        if is_port && (!h.contains(':') || h.starts_with('[')) {
            if let Ok(port) = p.parse::<u16>() {
                return (h.trim_matches(|c| c == '[' || c == ']').to_string(), port);
            }
        }
    }
    (
        s.trim_matches(|c| c == '[' || c == ']').to_string(),
        DEFAULT_PORT,
    )
}

fn describe(e: &std::io::Error) -> String {
    match e.kind() {
        ErrorKind::TimedOut | ErrorKind::WouldBlock => CODE_TIMEOUT.into(),
        ErrorKind::ConnectionRefused | ErrorKind::ConnectionReset => CODE_REFUSED.into(),
        _ => CODE_FAILED.into(),
    }
}

fn tcp_check(addrs: &[SocketAddr], timeout: Duration) -> Result<u64, String> {
    let mut last = CODE_FAILED.to_string();
    for addr in addrs {
        let t0 = Instant::now();
        match TcpStream::connect_timeout(addr, timeout) {
            Ok(_) => return Ok(t0.elapsed().as_millis() as u64),
            Err(e) => last = describe(&e),
        }
    }
    Err(last)
}

fn udp_request() -> Vec<u8> {
    let mut inner = vec![0x0a, UDP_PROBE_ID.len() as u8]; // RegisterPeer.id (field 1, string)
    inner.extend_from_slice(UDP_PROBE_ID);
    let mut msg = vec![0x32, inner.len() as u8]; // RendezvousMessage.register_peer (field 6, message)
    msg.extend(inner);
    msg
}

/// 送出請求並等待回應；在逾時的一半時重送一次（UDP 可能掉封包）。
fn udp_check(addrs: &[SocketAddr], timeout: Duration) -> Result<u64, String> {
    let req = udp_request();
    let mut last = CODE_NO_REPLY.to_string();
    for addr in addrs {
        let bind = if addr.is_ipv4() {
            "0.0.0.0:0"
        } else {
            "[::]:0"
        };
        let sock = match UdpSocket::bind(bind).and_then(|s| s.connect(addr).map(|_| s)) {
            Ok(s) => s,
            Err(e) => {
                last = describe(&e);
                continue;
            }
        };
        let _ = sock.set_read_timeout(Some(timeout / 2));
        let t0 = Instant::now();
        let mut buf = [0u8; 512];
        for _attempt in 0..2 {
            if let Err(e) = sock.send(&req) {
                last = describe(&e);
                break;
            }
            match sock.recv(&mut buf) {
                Ok(n) if n > 0 && UDP_REPLY_TAGS.contains(&buf[0]) => {
                    return Ok(t0.elapsed().as_millis() as u64)
                }
                Ok(_) => {
                    last = CODE_NOT_RUSTDESK.into();
                    break;
                }
                // Windows 收到 ICMP「埠不可達」時 recv 會回 ConnectionReset：代表主機在、但沒有服務
                Err(e)
                    if matches!(
                        e.kind(),
                        ErrorKind::ConnectionReset | ErrorKind::ConnectionRefused
                    ) =>
                {
                    last = CODE_REFUSED.into();
                    break;
                }
                Err(e) if matches!(e.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock) => {
                    last = CODE_NO_REPLY.into()
                }
                Err(e) => {
                    last = describe(&e);
                    break;
                }
            }
        }
    }
    Err(last)
}

fn check_all_protocols(host: &str, port: u16, timeout: Duration) -> Reach {
    let mut r = Reach {
        server: String::new(),
        ok: false,
        ms: None,
        error: None,
        udp_ok: false,
        udp_ms: None,
        udp_error: None,
    };
    let addrs: Vec<SocketAddr> = match (host, port).to_socket_addrs() {
        Ok(a) => a.collect(),
        Err(_) => vec![],
    };
    if addrs.is_empty() {
        r.error = Some(CODE_RESOLVE_FAILED.into());
        r.udp_error = Some(CODE_RESOLVE_FAILED.into());
        return r;
    }
    // TCP 與 UDP 並行，整體耗時以較慢者為準
    let (tcp, udp) = std::thread::scope(|s| {
        let t = s.spawn(|| tcp_check(&addrs, timeout));
        let u = s.spawn(|| udp_check(&addrs, timeout));
        (
            t.join().unwrap_or_else(|_| Err(CODE_INTERNAL.into())),
            u.join().unwrap_or_else(|_| Err(CODE_INTERNAL.into())),
        )
    });
    match tcp {
        Ok(ms) => {
            r.ok = true;
            r.ms = Some(ms);
        }
        Err(e) => r.error = Some(e),
    }
    match udp {
        Ok(ms) => {
            r.udp_ok = true;
            r.udp_ms = Some(ms);
        }
        Err(e) => r.udp_error = Some(e),
    }
    r
}

/// 名稱解析沒有逾時機制，所以整個檢查放到獨立執行緒，整體以 `timeout` 加緩衝為上限。
pub fn probe(server: &str, timeout: Duration) -> Reach {
    let (host, port) = split_host_port(server);
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(check_all_protocols(&host, port, timeout));
    });
    let mut r = rx
        .recv_timeout(timeout + Duration::from_secs(1))
        .unwrap_or_else(|_| Reach {
            server: String::new(),
            ok: false,
            ms: None,
            error: Some(CODE_TIMEOUT.into()),
            udp_ok: false,
            udp_ms: None,
            udp_error: Some(CODE_TIMEOUT.into()),
        });
    r.server = server.to_string();
    r
}

/// 同時檢查多個伺服器。
pub fn probe_all(servers: Vec<String>, timeout: Duration) -> Vec<Reach> {
    let handles: Vec<_> = servers
        .into_iter()
        .map(|s| std::thread::spawn(move || probe(&s, timeout)))
        .collect();
    handles.into_iter().filter_map(|h| h.join().ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn splits_host_and_port() {
        assert_eq!(
            split_host_port("203.0.113.1:21116"),
            ("203.0.113.1".into(), 21116)
        );
        assert_eq!(
            split_host_port("rustdesk.example.com"),
            ("rustdesk.example.com".into(), DEFAULT_PORT)
        );
        assert_eq!(split_host_port("  host:9999 "), ("host".into(), 9999));
        assert_eq!(split_host_port("[::1]:21116"), ("::1".into(), 21116));
        assert_eq!(split_host_port("::1"), ("::1".into(), DEFAULT_PORT)); // 裸 IPv6 不當成有埠
        assert_eq!(
            split_host_port("host:abc"),
            ("host:abc".into(), DEFAULT_PORT)
        );
        assert_eq!(
            split_host_port("host:99999"),
            ("host:99999".into(), DEFAULT_PORT)
        ); // 超出埠範圍
    }

    #[test]
    fn udp_request_matches_the_verified_wire_bytes() {
        // 與用真實伺服器實測成功的封包逐位元組相同
        assert_eq!(
            udp_request(),
            [&[0x32, 0x10, 0x0a, 0x0e][..], b"rdswitch-probe"].concat()
        );
    }

    /// 假的 hbbs：收到 RegisterPeer 就回 RegisterPeerResponse{request_pk:true}（與真實伺服器實測回應相同）。
    fn fake_hbbs(reply: &'static [u8]) -> (SocketAddr, std::thread::JoinHandle<()>) {
        let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        sock.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let addr = sock.local_addr().unwrap();
        let h = std::thread::spawn(move || {
            let mut buf = [0u8; 512];
            if let Ok((n, from)) = sock.recv_from(&mut buf) {
                if buf[..n] == udp_request()[..] {
                    let _ = sock.send_to(reply, from);
                }
            }
        });
        (addr, h)
    }

    #[test]
    fn udp_ok_only_for_valid_reply() {
        let (addr, h) = fake_hbbs(&[0x3a, 0x02, 0x10, 0x01]);
        let ms = udp_check(&[addr], Duration::from_secs(2)).expect("valid reply should be ok");
        assert!(ms < 2000);
        h.join().unwrap();

        let (addr, h) = fake_hbbs(b"HTTP/1.1 400 Bad Request"); // 不是 RustDesk 的回應
        assert_eq!(
            udp_check(&[addr], Duration::from_secs(2)),
            Err(CODE_NOT_RUSTDESK.to_string())
        );
        h.join().unwrap();
    }

    #[test]
    fn udp_silent_or_closed_port_fails() {
        // 有 socket 但不回應 -> 無回應
        let silent = UdpSocket::bind("127.0.0.1:0").unwrap();
        let r = udp_check(&[silent.local_addr().unwrap()], Duration::from_millis(600));
        assert_eq!(r, Err(CODE_NO_REPLY.to_string()));
        // 埠已關閉 -> 被拒絕（ICMP）或無回應，兩者都必須是失敗
        let addr = silent.local_addr().unwrap();
        drop(silent);
        assert!(udp_check(&[addr], Duration::from_millis(600)).is_err());
    }

    #[test]
    fn tcp_open_port_ok_and_closed_port_fails() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        assert!(tcp_check(&[addr], Duration::from_secs(2)).is_ok());
        drop(l);
        // 測試是平行執行的：別的測試可能剛好拿到這個剛釋放的埠（macOS 回收得特別快），
        // 所以換幾個埠重試；只要有一次確認「沒人在聽的埠會失敗」就算驗證到了。
        let mut closed_fails = tcp_check(&[addr], Duration::from_secs(2)).is_err();
        for _ in 0..5 {
            if closed_fails {
                break;
            }
            let l = TcpListener::bind("127.0.0.1:0").unwrap();
            let addr = l.local_addr().unwrap();
            drop(l);
            closed_fails = tcp_check(&[addr], Duration::from_secs(2)).is_err();
        }
        assert!(closed_fails, "沒人在聽的埠，連線應該失敗");
    }

    #[test]
    fn probe_reports_tcp_and_udp_independently() {
        // 只有 TCP 在聽、UDP 沒有服務：TCP ok、UDP 失敗（正是「半個在線」的情況）
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap().to_string();
        let r = probe(&addr, Duration::from_millis(800));
        assert!(r.ok && r.ms.is_some(), "{r:?}");
        assert!(!r.udp_ok && r.udp_error.is_some(), "{r:?}");
    }

    #[test]
    fn unresolvable_host_fails_cleanly() {
        // 約定：不論 DNS 是快速回報失敗（解析失敗），還是很慢（逾時），都要乾淨地失敗，且在時限內回來
        let t0 = Instant::now();
        let r = probe("nonexistent.invalid:21116", Duration::from_secs(3));
        assert!(
            t0.elapsed() < Duration::from_secs(5),
            "探測沒有在時限內結束：{:?}",
            t0.elapsed()
        );
        assert!(!r.ok && !r.udp_ok);
        for e in [r.error.as_deref(), r.udp_error.as_deref()] {
            assert!(
                matches!(e, Some(CODE_RESOLVE_FAILED) | Some(CODE_TIMEOUT)),
                "{e:?}"
            );
        }
    }

    #[test]
    fn probe_all_keeps_each_result() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let up = l.local_addr().unwrap().to_string();
        let rs = probe_all(
            vec![up.clone(), "nonexistent.invalid".into()],
            Duration::from_secs(2),
        );
        assert_eq!(rs.len(), 2);
        assert!(rs.iter().find(|r| r.server == up).unwrap().ok);
        assert!(
            !rs.iter()
                .find(|r| r.server == "nonexistent.invalid")
                .unwrap()
                .ok
        );
    }

    /// 對使用者真實設定的伺服器做一次實際檢查（唯讀；TCP 連線 + 一個最小的 UDP 請求）。
    #[test]
    #[ignore]
    fn probe_real_profiles() {
        let servers: Vec<String> = crate::profiles::load()
            .unwrap()
            .into_iter()
            .map(|p| p.server)
            .filter(|s| !s.is_empty())
            .collect();
        for r in probe_all(servers, Duration::from_secs(3)) {
            println!(
                "{} -> tcp ok={} ms={:?} err={:?} | udp ok={} ms={:?} err={:?}",
                r.server, r.ok, r.ms, r.error, r.udp_ok, r.udp_ms, r.udp_error
            );
            assert!(r.ok && r.udp_ok, "{r:?}");
        }
    }
}
