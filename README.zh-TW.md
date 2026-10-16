# rdswitch

[English](README.md) · [繁體中文](README.zh-TW.md)

系統匣常駐的 [RustDesk](https://rustdesk.com) 伺服器切換工具：一鍵切換 ID／中繼／API 伺服器與公鑰。使用 [Tauri 2](https://tauri.app)（Rust 核心 + 作業系統內建 WebView），目標平台為 Windows、macOS、Linux。

> **非官方專案。** 本專案與 RustDesk 無任何隸屬、背書或贊助關係。
> 狀態：目前只在 Windows 驗證過，詳見[已知限制](#已知限制)。

## 下載

到 [Releases](https://github.com/shen-labs/rdswitch/releases/latest) 頁面下載最新版，依系統選擇檔案：

| 系統 | 檔案 | 備註 |
|---|---|---|
| Windows | `rdswitch_<版本>_x64-setup.exe` | 每位使用者各自安裝。未做程式碼簽署，SmartScreen 會出現警告。 |
| macOS | `rdswitch_<版本>_universal.dmg` | Intel 與 Apple 晶片通用。未經公證，Gatekeeper 會出現警告。尚未在實體 Mac 上測試。 |
| Linux | `.deb`（Debian/Ubuntu）、`.rpm`（Fedora/RHEL）、`.AppImage`（其他） | 尚未在實體機器上測試。 |

`.sig` 檔是應用程式內更新使用的簽章，不需要下載。

## 功能

- **一鍵切換**：從系統匣選單或主視窗切換。每個 profile 包含 ID 伺服器、中繼伺服器、API 伺服器與公鑰；欄位留空代表「RustDesk 預設」。
- **Default**：內建且唯讀，代表 RustDesk 官方預設。
- **我的最愛**：加上星號的 profile 會排在清單最前面，並直接列在系統匣選單的第一層（其餘收進子選單）。
- **伺服器狀態**：每一列顯示 ID 伺服器是否連得上（21116 埠的 TCP **與** UDP）與延遲，滑鼠停留可看詳細數據。
- **貼上自動填入**：在新增／編輯對話框貼上 RustDesk 設定字串、JSON，或「ID 伺服器: … / Key: …」文字，欄位會自動填好。
- **多國語言**：English、繁體中文、简体中文、日本語；預設跟隨系統語言，可在設定中更改。新增語言只需在兩邊各補一份字典（見[架構](#架構)）。
- **響應式版面**，亮色／暗色主題跟隨系統。
- **開機自動啟動**（選用）。
- **應用程式內更新**（選用，見[更新](#更新)）。

系統匣選單（右鍵開選單；左鍵開主視窗）：

```
  目前：公司                              ← 灰色、不可點
────────────────────────
✓ 公司                                    ← 我的最愛直接列出（✓ = 使用中）
  家裡
  其他 ▸                                  ← 其餘收進子選單；沒有最愛時全部直接列出
────────────────────────
  開啟主視窗
✓ 開機自動啟動
────────────────────────
  結束
```

## 切換如何生效

RustDesk 若已安裝為**系統服務**，執行中的服務行程把設定放在記憶體，所以**只改設定檔不會生效**。因此本程式這樣做：

| RustDesk 安裝型態 | 做法 | 需要權限 |
|---|---|---|
| 已安裝為系統服務 | 以 `rustdesk --option <key> <value>` 通知執行中的服務，並讀回驗證 | 系統管理員／root |
| 免安裝／無服務 | 關閉 GUI → 備份 → 原子寫入 `RustDesk2.toml` → 重啟 GUI | 一般使用者 |

Windows 版本帶有 `requireAdministrator` manifest，啟動時會跳 UAC。同樣的原因，「開機自動啟動」使用最高權限的排程工作（一般的 `Run` 登錄項目無法在登入時啟動需提升權限的程式）。

切換的設定：`custom-rendezvous-server`、`relay-server`、`api-server`、`key`。每次變更前都會把原本的設定備份到本程式的資料夾。

## 伺服器狀態

對每個 profile 的 ID 伺服器，程式會並行做兩項檢查：

- **TCP**：連線到 21116 埠（預設）並量測時間。
- **UDP**：UDP 沒有連線概念，所以改為送出一個最小的 RustDesk 協定請求（`RegisterPeer`，使用用完即丟的假 ID `rdswitch-probe`）並驗證回應。伺服器會回 `RegisterPeerResponse`，且不會儲存這個假 ID。封包格式依循 RustDesk 公開的 `rendezvous.proto`，並已用真實的 `hbbs` 伺服器驗證；本專案不包含任何 RustDesk 原始碼。

標籤只顯示一個延遲（TCP 連線時間）；圓點顏色綜合兩項結果：綠＝兩者都通、黃＝只有 TCP 通、紅＝TCP 不通。滑鼠停留可看兩項的詳細數據。檢查時機：新增 profile、視窗取得焦點、視窗可見時每 30 秒、以及按重新整理鈕。Default 沒有固定位址，不檢查。

只檢查 ID 伺服器；中繼（21117）、NAT 測試（21115）與 API 埠都沒有檢查。

## 更新

程式可以檢查是否有新版本並直接就地安裝，不需要自己到 GitHub 下載安裝檔。

- 啟動後幾秒、之後每 6 小時，程式會向發佈來源查詢是否有較新的版本。有的話，視窗上方會出現橫幅，顯示新版本與更新內容。**沒有你的點選就不會安裝任何東西**：選「立即更新」（下載、驗證簽章、安裝，然後程式重新啟動），或選「稍後」（收起橫幅，下次啟動才會再提醒）。
- 每個更新都有簽章。程式只會安裝簽章與內嵌公鑰相符的更新；簽章不正確的更新會被拒絕。發佈清單本身（`latest.json`）*沒有*簽章，所以程式也要求簽章裡帶有「簽章時的版本號」（`requireSignedVersion`），並在它與清單宣告的版本不一致時拒絕更新。這能擋下偽造的清單把「很高的版本號」配上「舊版、真的有簽章但已過時」的安裝檔。
- 檢查更新只會透過 HTTPS 連到設定的發佈來源（GitHub Releases），沒有任何遙測。
- 只有在 `plugins.updater.pubkey` 與 `plugins.updater.endpoints` 都設定好時，更新功能才會運作；本儲存庫兩者都已設定，所以官方版本會檢查更新。如果缺任何一項（例如 fork 後還沒設定自己的金鑰），更新功能會維持關閉，設定中會顯示「尚未設定更新來源」。見[發佈與啟用更新](#發佈與啟用更新)。

## 隱私

- 沒有遙測、分析或帳號。
- Profile（含公鑰）以純文字存放在程式的資料夾（macOS／Linux 的檔案權限為 `0600`），不會離開你的電腦。
- 唯一的網路流量：對*你設定的伺服器*做 TCP／UDP 狀態檢查，以及——若已設定更新——向發佈來源檢查更新。

## 資料位置

| 作業系統 | 資料夾 |
|---|---|
| Windows | `%APPDATA%\rdswitch` |
| macOS | `~/Library/Application Support/rdswitch` |
| Linux | `~/.config/rdswitch` |

檔案：`profiles.json`、`favorites.json`、`settings.json`（語言）、`backups/`。

## 建置

需求：Node 18+、[rustup](https://rustup.rs)（精確的 Rust 版本與 `rustfmt`、`clippy` 都鎖定在 `rust-toolchain.toml`，用 `rustup install` 即可安裝），以及你的作業系統所需的 [Tauri 前置套件](https://tauri.app/start/prerequisites/)（Linux：`libwebkit2gtk-4.1-dev libayatana-appindicator3-dev`）。

```
npm install
npm run tauri dev                 # 開發（Windows 需在系統管理員終端機）
cargo test --locked --manifest-path src-tauri/Cargo.toml
node scripts/check-i18n.mjs      # 前端翻譯字典的一致性
node scripts/check-version.mjs   # 五個檔案的版本號必須一致
npm run tauri build               # 產生安裝檔：nsis、dmg、deb/rpm/AppImage
```

一般的 `cargo test` 不會碰真實的 RustDesk。下面的端到端驗證**會暫時改動真實的 RustDesk 設定**，且需要系統管理員／root：

```
RDS_E2E_PROFILE=<已存在的 profile 名稱> cargo test -- --ignored --nocapture --test-threads=1
```

> **如果你要自行散佈編譯的版本**，請把本機路徑改寫掉，以免它們被內嵌進執行檔（會出現在錯誤訊息中）：
> `RUSTFLAGS="--remap-path-prefix=$PWD=/build --remap-path-prefix=$HOME/.cargo=/cargo --remap-path-prefix=$HOME/.rustup=/rustup"`。
> 發佈用的工作流程已經這樣做。

## 發佈與啟用更新

推送 `v*` tag 時，`.github/workflows/release.yml` 會為三個平台編譯安裝檔。要啟用應用程式內更新，維護者需要：

1. 產生簽章金鑰組（只做一次），要放在**專案資料夾之外**，並在一般終端機執行（它會要你設定密碼）：`npx tauri signer generate -w ~/.tauri/rdswitch.key`。**絕對不要提交私鑰**（`*.key` 已加入 `.gitignore` 當作保險）。私鑰遺失後，既有的安裝就無法再更新。
2. 把**公鑰**（`rdswitch.key.pub`）填入 `src-tauri/tauri.conf.json` → `plugins.updater.pubkey`。本儲存庫已經填入官方發佈金鑰的公鑰；fork 的人必須用自己的。
3. `plugins.updater.endpoints` 已經指向 `https://github.com/shen-labs/rdswitch/releases/latest/download/latest.json`。如果你 fork 這個專案，請改成你自己的儲存庫。
4. 在儲存庫新增 secrets：`TAURI_SIGNING_PRIVATE_KEY`（金鑰內容），若有設定密碼則加上 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`。
5. 更新版本號（`package.json`、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json`），推送 `v*` tag，並**發佈**工作流程建立的草稿 release。用戶端只看得到已發佈、非草稿、非預先發行的 release。

正式宣布前，請用真實的 release 完整測試一次：安裝版本 *N*、發佈 *N+1*，確認橫幅出現且更新能順利安裝。（下面的自動化測試涵蓋簽章檢查，**不包含**真實的發佈來源，以及安裝與重新啟動這一步。）

**簽章測試（Windows x64）。** `src-tauri/tests/updater_e2e.rs` 會讓真正的更新外掛對著本機的假發佈來源與一個真實簽章過的安裝檔運作，全程不安裝任何東西。它檢查：真實的更新能下載並逐位元組驗證通過；檔案被竄改、金鑰不符、假造較高的版本號都會被拒絕。沒有設定下列環境變數時會自動略過：

```
npx tauri signer generate --ci -w /tmp/test.key            # 一次性的測試金鑰，不是你的發佈金鑰
npx tauri signer generate --ci -w /tmp/other.key           # 另一組不相干的金鑰
# 用測試金鑰打包出簽章過的安裝檔（TAURI_SIGNING_PRIVATE_KEY=<金鑰內容>，加上
# --config src-tauri/tauri.release.conf.json，以及把 plugins.updater.pubkey 設為 /tmp/test.key.pub 的設定）
RDS_UPDATER_TEST_EXE=<簽章過的 *-setup.exe 路徑> \
RDS_UPDATER_TEST_PUB=/tmp/test.key.pub \
RDS_UPDATER_TEST_OTHER_PUB=/tmp/other.key.pub \
cargo test --manifest-path src-tauri/Cargo.toml --test updater_e2e -- --nocapture
```

另可設定 `RDS_UPDATER_TEST_UNVERSIONED_EXE`，指向用 `npx tauri signer sign` 簽的安裝檔副本（它不會記錄版本號），就能一併示範 `requireSignedVersion` 防的是什麼。

## 架構

```
src/                  前端（純 HTML/CSS/JS，無打包器）：頂部工具列 + 狀態列 + 清單
  i18n.js             前端字典（介面文字）
src-tauri/src/
  platform.rs         平台適配層：路徑、權限、RustDesk 路徑與服務偵測、--option、
                      開機啟動、系統語言（唯一判斷作業系統的地方）
  tomledit.rs         只改 RustDesk2.toml 的 [options]，其餘內容原樣保留
  parse.rs            「貼上設定文字」的解析（設定字串 / JSON / 標籤文字）
  probe.rs            伺服器狀態：TCP 連線 + UDP 協定探測（名稱解析有逾時保護）
  profiles.rs         profile 與最愛：儲存、Default 保護、套用
  i18n.rs             後端字典（系統匣選單、錯誤訊息）
  settings.rs         使用者設定（語言）
  lib.rs              系統匣選單、Tauri 命令、單一執行個體、關閉視窗縮到系統匣
src-tauri/windows-admin.manifest   Windows 以系統管理員執行
```

新增語言：在 `i18n.rs` 的 `Lang`／`ALL`／`TABLE`、`src/i18n.js` 的 `LANGS`／`DICT`，以及語言選單各補一項。兩份字典都有一致性檢查（每個語言都要有每個鍵，且變數一致）。

## 與 RustDesk 的關係

- 這是獨立、非官方的工具。「RustDesk」是他人擁有的專案與產品名稱；本專案沒有使用 RustDesk 的標誌或美術素材，名稱刻意取為 `rdswitch` 而不是以 RustDesk 命名，「RustDesk」只用來說明這個工具搭配的對象。我找不到 RustDesk 公開的商標使用規定，但「沒有規定」不等於「允許」；如果不確定，請詢問 RustDesk 團隊。
- RustDesk 用戶端以 AGPL-3.0 授權釋出。本專案不包含它的任何原始碼；只是修改 RustDesk 的設定、呼叫它的命令列（`rustdesk --option`），並對你設定的伺服器送出一個很小的封包。這不是法律意見。
- RustDesk 以服務執行時，`rustdesk --option <key> <value>` 是讓切換生效的關鍵，但官方公開的用戶端設定文件只描述了 `rustdesk --config <字串>`，我找不到 `--option` 的文件。它在目前測試過的版本可以運作，但不是有文件保證的介面，之後可能改變。

## 已知限制

- 目前只在 Windows 驗證過。macOS／Linux 的設定路徑、`--option`、服務偵測，以及關閉與啟動 RustDesk 的方式都沒在實機測過；那兩個平台在已安裝服務時需要 root，請用 `sudo` 啟動本程式。
- 安裝檔沒有程式碼簽章（Windows Authenticode／macOS 公證），作業系統會顯示安全警告。
- RustDesk 以服務執行時，每次切換都需要系統管理員權限（Windows 為啟動時的 UAC）；目前沒有獨立的特權輔助程式。
- UDP、中繼與 API 的連線檢查只做了一部分，見[伺服器狀態](#伺服器狀態)。

## 授權

[MIT](LICENSE) © 2026 SHEN。第三方相依套件（Tauri 等）各自保有自己的授權；散佈執行檔時請附上它們的授權聲明（例如用 `cargo about` 產生）。
