//! 更新流程的整合測試：用真實簽章過的安裝檔、本機假伺服器與 mock 應用程式，
//! 讓更新外掛實際走一遍「檢查 → 下載 → 驗證簽章」。不會安裝任何東西。
//!
//! 需要由 `tauri build` 產生並簽章的安裝檔，以及簽章金鑰與另一組不相干金鑰的公鑰檔（見環境變數）。
//! 沒有設定環境變數時會直接略過（不算失敗），所以一般的 `cargo test` 不受影響。
//!
//!   RDS_UPDATER_TEST_EXE        安裝檔路徑（旁邊要有 <安裝檔>.sig）
//!   RDS_UPDATER_TEST_PUB        簽章用金鑰的公鑰檔（*.key.pub）
//!   RDS_UPDATER_TEST_OTHER_PUB  另一組不相干金鑰的公鑰檔
//!   RDS_UPDATER_TEST_UNVERSIONED_EXE（選用）同一個安裝檔的副本，旁邊的 .sig 是用 `tauri signer sign`
//!                               簽的、簽章裡沒有版本資訊（用來測 requireSignedVersion 的作用）

use serde_json::json;
use std::path::PathBuf;
use tauri::test::{mock_builder, mock_context, noop_assets};
use tauri_plugin_updater::UpdaterExt;

type Routes = Vec<(&'static str, Vec<u8>)>;

/// 本機假的更新伺服器（用成熟的 tiny_http）：依路徑回傳固定內容，其餘回 404。
fn serve(server: tiny_http::Server, routes: Routes) {
    std::thread::spawn(move || {
        for request in server.incoming_requests() {
            let path = request.url().to_string();
            let response = match routes.iter().find(|(p, _)| *p == path) {
                Some((_, body)) => request.respond(tiny_http::Response::from_data(body.clone())),
                None => request.respond(tiny_http::Response::empty(404)),
            };
            if let Err(e) = response {
                eprintln!("[假伺服器] 回應 {path} 失敗：{e}");
            }
        }
    });
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var(name).ok().map(PathBuf::from)
}

/// 以本機假伺服器發佈一份更新清單，並讓更新外掛實際「檢查」；回傳找到的更新。
/// （app 要一併回傳，確保 mock 應用程式在測試期間一直存活。）
fn fetch_update(
    pubkey: &str,
    announced_version: &str,
    require_signed_version: bool,
    payload: &[u8],
    signature: &str,
) -> Result<
    (
        tauri::App<tauri::test::MockRuntime>,
        tauri_plugin_updater::Update,
    ),
    String,
> {
    let server = tiny_http::Server::http("127.0.0.1:0").expect("啟動假伺服器");
    let port = server.server_addr().to_ip().expect("伺服器位址").port();
    let manifest = json!({
        "version": announced_version,
        "notes": "test",
        "pub_date": "2026-01-01T00:00:00Z",
        "platforms": { "windows-x86_64": { "signature": signature, "url": format!("http://127.0.0.1:{port}/update.exe") } }
    });
    serve(
        server,
        vec![
            ("/latest.json", manifest.to_string().into_bytes()),
            ("/update.exe", payload.to_vec()),
        ],
    );

    let mut ctx = mock_context(noop_assets());
    ctx.package_info_mut().version = "0.0.1".parse().unwrap(); // 讓宣告的版本比「目前版本」新
    ctx.config_mut().plugins.0.insert(
        "updater".into(),
        json!({
            "pubkey": pubkey.trim(),
            "endpoints": [format!("http://127.0.0.1:{port}/latest.json")],
            "dangerousInsecureTransportProtocol": true, // 只有這個測試為了連本機假伺服器才開
            "requireSignedVersion": require_signed_version,
        }),
    );
    let app = mock_builder()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .build(ctx)
        .map_err(|e| e.to_string())?;
    let handle = app.handle().clone();
    let update = tauri::async_runtime::block_on(async move {
        handle
            .updater()
            .map_err(|e| e.to_string())?
            .check()
            .await
            .map_err(|e| format!("check: {e} :: {e:?}"))
    })?
    .ok_or_else(|| "check: 沒有可用的更新".to_string())?;
    Ok((app, update))
}

#[test]
fn signed_update_is_verified_and_tampering_is_rejected() {
    let (Some(exe), Some(pub_path), Some(other_pub_path)) = (
        env_path("RDS_UPDATER_TEST_EXE"),
        env_path("RDS_UPDATER_TEST_PUB"),
        env_path("RDS_UPDATER_TEST_OTHER_PUB"),
    ) else {
        eprintln!(
            "略過：未設定 RDS_UPDATER_TEST_EXE / RDS_UPDATER_TEST_PUB / RDS_UPDATER_TEST_OTHER_PUB"
        );
        return;
    };
    let payload = std::fs::read(&exe).expect("讀取安裝檔");
    let sig = std::fs::read_to_string(format!("{}.sig", exe.display())).expect("讀取 .sig");
    let pubkey = std::fs::read_to_string(pub_path).expect("公鑰");
    let other_pub = std::fs::read_to_string(other_pub_path).expect("另一組公鑰");
    let signed_version = env!("CARGO_PKG_VERSION"); // 安裝檔就是用這個版本簽出來的

    // verify() 做的檢查與 download()/install() 完全相同（簽章 + 簽章綁定的版本），只是不發請求。

    // 1. 正常：簽章正確、版本一致 → 通過
    let (_app, update) =
        fetch_update(&pubkey, signed_version, true, &payload, &sig).expect("check 應該找到更新");
    assert_eq!(update.version, signed_version);
    update.verify(&payload).expect("正常的更新應該通過驗證");
    println!("[1] 正常更新：通過驗證（{} bytes）", payload.len());

    // 2. 檔案被竄改一個位元組 → 必須被拒絕
    let mut tampered = payload.clone();
    let mid = tampered.len() / 2;
    tampered[mid] ^= 0x01;
    let err = update
        .verify(&tampered)
        .expect_err("被竄改的更新必須被拒絕");
    println!("[2] 竄改一個位元組：被拒絕 -> {err}");

    // 3. 用不相干的金鑰驗證（等於簽章不是我們的金鑰簽的）→ 必須被拒絕
    let (_app3, update3) =
        fetch_update(&other_pub, signed_version, true, &payload, &sig).expect("check 應該找到更新");
    let err = update3
        .verify(&payload)
        .expect_err("簽章金鑰不符必須被拒絕");
    println!("[3] 錯誤的金鑰：被拒絕 -> {err}");

    // 4. 降級攻擊：清單（本身沒有簽章）宣告很高的版本號，卻配上真實但舊版的檔案與合法簽章。
    //    只要簽章帶有版本資訊，版本不符就一律被拒絕（與 requireSignedVersion 無關）
    for flag in [true, false] {
        let (_a, u) =
            fetch_update(&pubkey, "9.9.9", flag, &payload, &sig).expect("check 應該找到更新");
        let err = u
            .verify(&payload)
            .expect_err("版本號與簽章綁定的版本不符必須被拒絕");
        println!("[4] 假造高版本號（降級攻擊，requireSignedVersion={flag}）：被拒絕 -> {err}");
    }

    // 5. requireSignedVersion 的真正作用：簽章「沒有版本資訊」時（舊式簽章，或攻擊者手上沒綁版本的舊簽章）
    //    - 旗標關閉：版本檢查被直接略過 → 假造版本號的攻擊會成功（殘留風險）
    //    - 旗標開啟：這種簽章一律被拒絕
    let Some(unv) = env_path("RDS_UPDATER_TEST_UNVERSIONED_EXE") else {
        eprintln!("[5] 略過：未設定 RDS_UPDATER_TEST_UNVERSIONED_EXE");
        return;
    };
    let unv_payload = std::fs::read(&unv).expect("讀取副本");
    let unv_sig =
        std::fs::read_to_string(format!("{}.sig", unv.display())).expect("讀取副本的 .sig");
    let (_a, off) =
        fetch_update(&pubkey, "9.9.9", false, &unv_payload, &unv_sig).expect("check 應該找到更新");
    off.verify(&unv_payload)
        .expect("旗標關閉：沒有版本資訊的簽章會繞過版本檢查");
    println!(
        "[5a] 沒有版本資訊的簽章 + 假造版本號 + 旗標關閉：通過驗證（這就是旗標要防的殘留風險）"
    );
    let (_a, on) =
        fetch_update(&pubkey, "9.9.9", true, &unv_payload, &unv_sig).expect("check 應該找到更新");
    let err = on
        .verify(&unv_payload)
        .expect_err("旗標開啟：沒有版本資訊的簽章必須被拒絕");
    println!("[5b] 同樣的情況 + 旗標開啟：被拒絕 -> {err}");
}

/// 診斷用：確認「下載」這段傳輸本身可以運作。用一個很小的、沒有簽章的檔案：
/// 傳輸成功的話，錯誤應該來自簽章驗證（而不是網路／解碼錯誤）。
#[test]
fn download_transport_works_with_a_small_payload() {
    let Some(pub_path) = env_path("RDS_UPDATER_TEST_PUB") else {
        eprintln!("略過：未設定 RDS_UPDATER_TEST_PUB");
        return;
    };
    let pubkey = std::fs::read_to_string(pub_path).expect("公鑰");
    let tiny = b"not a real installer".to_vec();
    let (_app, update) = fetch_update(
        &pubkey,
        "9.9.9",
        false,
        &tiny,
        "bm90IGEgcmVhbCBzaWduYXR1cmU=",
    )
    .expect("check");
    let err = tauri::async_runtime::block_on(update.download(|_, _| {}, || {}))
        .expect_err("沒有合法簽章的檔案不可能通過");
    println!("小檔案下載結果：{err}");
    let msg = err.to_string().to_lowercase();
    assert!(
        !msg.contains("decoding") && !msg.contains("network") && !msg.contains("timed out"),
        "傳輸本身失敗：{err}"
    );
}

/// 完整的端到端：透過本機伺服器下載整個簽章過的安裝檔，驗證簽章，內容必須與原檔逐位元組相同。
#[test]
fn full_download_of_the_signed_installer_verifies() {
    let (Some(exe), Some(pub_path)) = (
        env_path("RDS_UPDATER_TEST_EXE"),
        env_path("RDS_UPDATER_TEST_PUB"),
    ) else {
        eprintln!("略過：未設定 RDS_UPDATER_TEST_EXE / RDS_UPDATER_TEST_PUB");
        return;
    };
    let payload = std::fs::read(&exe).expect("讀取安裝檔");
    let sig = std::fs::read_to_string(format!("{}.sig", exe.display())).expect("讀取 .sig");
    let pubkey = std::fs::read_to_string(pub_path).expect("公鑰");

    let (_app, update) = fetch_update(&pubkey, env!("CARGO_PKG_VERSION"), true, &payload, &sig)
        .expect("check 應該找到更新");
    let mut chunks = 0usize;
    let got = tauri::async_runtime::block_on(update.download(|_, _| chunks += 1, || {}))
        .expect("完整下載並驗證簽章");
    assert_eq!(got, payload, "下載內容必須與簽章過的安裝檔完全一致");
    println!(
        "完整下載：{} bytes（{chunks} 個區塊）簽章驗證通過，與原檔一致",
        got.len()
    );
}
