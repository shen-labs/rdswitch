//! 提升權限執行 `rustdesk --option` 的指令組裝與結果解析。
//!
//! 這裡全是純函式、不判斷作業系統、不啟動行程，方便測試；實際呼叫 osascript / pkexec 在 platform.rs。
//!
//! 安全重點：值會被放進 shell 腳本，再（macOS）放進 AppleScript 字串，由系統以管理員身分執行。
//! 所以每一層都要正確跳脫，並且拒絕控制字元與不合法的 option 名稱，不信任任何輸入。

use crate::i18n::tr;

/// 腳本輸出中，分隔「設定指令的輸出」與「讀回的值」的標記。
pub const MARKER: &str = "__RDSWITCH_READBACK__";

/// 以單引號包住字串給 POSIX shell；字串內的單引號寫成 `'\''`。
pub fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// 產生 AppleScript 字串常值（含外層雙引號）：跳脫反斜線與雙引號。
pub fn applescript_quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', r"\\").replace('"', "\\\""))
}

/// option 名稱只允許小寫英文字母與連字號（例如 `custom-rendezvous-server`）。
fn valid_key(k: &str) -> bool {
    !k.is_empty() && k.chars().all(|c| c.is_ascii_lowercase() || c == '-')
}

/// 換行、NUL 等控制字元會讓腳本被截斷或夾帶指令，一律拒絕。
fn clean(s: &str) -> bool {
    !s.chars().any(|c| c.is_control())
}

/// 組出要以管理員身分執行的 shell 腳本：依序設定每個 option，最後讀回 `readback_key`。
/// 任何一個設定失敗就中止（`set -e`），不會在半套狀態下繼續。
pub fn build_script(
    exe: &str,
    changes: &[(&str, &str)],
    readback_key: &str,
) -> Result<String, String> {
    if !clean(exe) || exe.is_empty() || !valid_key(readback_key) {
        return Err(tr("err.elevate_bad_input"));
    }
    let mut parts = vec!["set -e".to_string()];
    for (key, value) in changes {
        if !valid_key(key) || !clean(value) {
            return Err(tr("err.elevate_bad_input"));
        }
        parts.push(format!(
            "{} --option {} {}",
            sh_quote(exe),
            sh_quote(key),
            sh_quote(value)
        ));
    }
    parts.push(format!("echo {MARKER}"));
    parts.push(format!(
        "{} --option {}",
        sh_quote(exe),
        sh_quote(readback_key)
    ));
    Ok(parts.join("; "))
}

/// macOS：`osascript -e` 的參數，會跳出系統密碼視窗。
pub fn osascript_arg(script: &str) -> String {
    format!(
        "do shell script {} with administrator privileges",
        applescript_quote(script)
    )
}

/// 從腳本輸出取出讀回的值。找不到標記、或值不只一行，回傳 None（呼叫端略過驗證，不誤報）。
pub fn parse_readback(output: &str) -> Option<String> {
    let after = output.split_once(MARKER)?.1.trim();
    (!after.contains('\n')).then(|| after.to_string())
}

/// 使用者是否在密碼視窗按了取消（macOS：錯誤碼 -128；pkexec：結束碼 126 = 對話框被關閉）。
pub fn is_cancelled(code: Option<i32>, stderr: &str) -> bool {
    let s = stderr.to_lowercase();
    code == Some(126) || s.contains("(-128)") || s.contains("user canceled")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sh_quote_wraps_and_escapes_single_quotes() {
        assert_eq!(sh_quote("abc"), "'abc'");
        assert_eq!(sh_quote(""), "''");
        assert_eq!(sh_quote("it's"), r"'it'\''s'");
    }

    /// 用真正的 /bin/sh 驗證：不論內容多惡意，都會原封不動地當成「一個參數」。
    #[cfg(unix)]
    #[test]
    fn sh_quote_survives_a_real_shell() {
        let nasty = [
            "plain",
            "a b  c",
            "'; echo INJECTED #",
            "$(echo INJECTED)",
            "`echo INJECTED`",
            "x\" ; echo INJECTED ; \"",
            r"back\slash\\",
            "semi;colon && pipe | amp &",
            "'''",
            "中文 空白",
        ];
        for s in nasty {
            let out = std::process::Command::new("/bin/sh")
                .arg("-c")
                .arg(format!("printf '%s' {}", sh_quote(s)))
                .output()
                .unwrap();
            assert_eq!(String::from_utf8_lossy(&out.stdout), s, "輸入：{s}");
        }
    }

    #[test]
    fn applescript_quote_escapes_backslash_and_quote() {
        assert_eq!(applescript_quote("abc"), "\"abc\"");
        assert_eq!(applescript_quote("a\"b"), "\"a\\\"b\"");
        assert_eq!(applescript_quote(r"a\b"), r#""a\\b""#);
        // 先處理反斜線再處理引號：  \"  必須變成  \\\"
        assert_eq!(applescript_quote("\\\""), r#""\\\"""#);
    }

    #[test]
    fn script_sets_each_option_then_reads_back() {
        let s = build_script(
            "/Applications/RustDesk.app/Contents/MacOS/RustDesk",
            &[
                ("custom-rendezvous-server", "203.0.113.5"),
                ("relay-server", ""),
            ],
            "custom-rendezvous-server",
        )
        .unwrap();
        assert!(s.starts_with("set -e; "));
        assert!(s.contains("--option 'custom-rendezvous-server' '203.0.113.5'"));
        assert!(s.contains("--option 'relay-server' ''"));
        assert!(s.ends_with("--option 'custom-rendezvous-server'"));
        assert!(s.contains(&format!("echo {MARKER}")));
    }

    #[test]
    fn script_rejects_control_characters_and_bad_keys() {
        let exe = "/usr/bin/rustdesk";
        let ok = [("relay-server", "x")];
        assert!(build_script(exe, &ok, "relay-server").is_ok());
        // 換行 / NUL / 其他控制字元
        for bad in ["a\nb", "a\rb", "a\0b", "a\tb", "a\u{1b}b"] {
            assert!(build_script(exe, &[("relay-server", bad)], "relay-server").is_err());
            assert!(build_script(bad, &ok, "relay-server").is_err());
        }
        // option 名稱：夾帶指令、大寫、空字串
        for key in ["relay-server; rm -rf /", "Relay", "", "a b", "k'x", "k$(x)"] {
            assert!(
                build_script(exe, &[(key, "v")], "relay-server").is_err(),
                "{key}"
            );
            assert!(build_script(exe, &ok, key).is_err(), "{key}");
        }
        assert!(build_script("", &ok, "relay-server").is_err());
    }

    /// 惡意的值放進完整腳本後，用真正的 sh 執行，確認不會執行夾帶的指令。
    #[cfg(unix)]
    #[test]
    fn malicious_value_cannot_inject_a_command_through_the_full_script() {
        let dir = std::env::temp_dir().join(format!("rds-elevate-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let fake = dir.join("fake rustdesk"); // 路徑含空白，同時驗證 exe 的跳脫
        std::fs::write(&fake, "#!/bin/sh\necho \"ARGS:$*\"\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let evidence = dir.join("INJECTED");

        let nasty = format!("x'; touch '{}'; echo '", evidence.display());
        let script = build_script(
            &fake.to_string_lossy(),
            &[("custom-rendezvous-server", &nasty)],
            "custom-rendezvous-server",
        )
        .unwrap();
        let out = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(&script)
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        assert!(out.status.success(), "{text}");
        assert!(!evidence.exists(), "夾帶的指令被執行了");
        assert!(text.contains("ARGS:--option custom-rendezvous-server x'; touch"));
        assert!(parse_readback(&text).is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn set_e_stops_after_the_first_failure() {
        // 第一個指令失敗時，後面的指令（含讀回）不應執行
        #[cfg(unix)]
        {
            let s = build_script(
                "/nonexistent/rustdesk",
                &[("relay-server", "x")],
                "relay-server",
            )
            .unwrap();
            let out = std::process::Command::new("/bin/sh")
                .arg("-c")
                .arg(&s)
                .output()
                .unwrap();
            assert!(!out.status.success());
            assert!(!String::from_utf8_lossy(&out.stdout).contains(MARKER));
        }
    }

    #[test]
    fn osascript_arg_embeds_the_quoted_script() {
        let a = osascript_arg("set -e; echo \"hi\"");
        assert_eq!(
            a,
            "do shell script \"set -e; echo \\\"hi\\\"\" with administrator privileges"
        );
    }

    #[test]
    fn readback_parsing() {
        assert_eq!(
            parse_readback(&format!("noise\n{MARKER}\n203.0.113.5\n")),
            Some("203.0.113.5".into())
        );
        // 清除成預設值：標記後面是空的
        assert_eq!(parse_readback(&format!("{MARKER}\n")), Some(String::new()));
        // 沒有標記、或值有多行：不驗證
        assert_eq!(parse_readback("203.0.113.5"), None);
        assert_eq!(parse_readback(&format!("{MARKER}\na\nb\n")), None);
    }

    #[test]
    fn cancel_detection() {
        assert!(is_cancelled(
            Some(1),
            "execution error: User canceled. (-128)"
        ));
        assert!(is_cancelled(Some(126), ""));
        assert!(!is_cancelled(
            Some(1),
            "execution error: not allowed (-60007)"
        ));
        assert!(!is_cancelled(
            Some(127),
            "Error executing command as another user"
        ));
    }
}
