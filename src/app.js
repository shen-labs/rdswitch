const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const $ = (id) => document.getElementById(id);
const FIELDS = ["name", "server", "relay", "api", "key"];
const esc = (s) => String(s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));

const svg = (body, size = 18) =>
  `<svg viewBox="0 0 24 24" width="${size}" height="${size}" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${body}</svg>`;
const ICON = {
  star: svg('<path d="M12 3l2.7 5.6 6.1.9-4.4 4.3 1 6.1L12 17l-5.4 2.9 1-6.1L3.2 9.5l6.1-.9z"/>', 20),
  dots: svg('<circle cx="12" cy="5" r="1.4"/><circle cx="12" cy="12" r="1.4"/><circle cx="12" cy="19" r="1.4"/>', 20),
};

let state = { profiles: [], current: "", lang: "en", lang_setting: "auto" };
const reach = new Map(); // 伺服器位址 -> 最近一次的連線檢查結果
let checking = false;
let lastCheck = 0;
let busy = false;
let menuFor = null; // 目前開啟「更多」選單的 profile 名稱
let menuBtn = null;
let armedDelete = false;
let dismissedVersion = null; // 使用者按「稍後」的版本；本次執行期間不再顯示橫幅
let updating = false; // 正在下載／安裝更新
let updateError = null;
let editingOld = null; // 編輯中的原名稱（新增時為 null），改名時後端用它找到原本那一筆

// 先用瀏覽器語言畫出初始畫面；後端回報有效語言後（get_state）再修正。
setLang(langFromLocale(navigator.language));
applyStaticI18n();

// ---------- Toast ----------
let toastTimer;
function toast(text, err) {
  const el = $("toast");
  clearTimeout(toastTimer);
  if (!text) {
    el.hidden = true;
    return;
  }
  el.textContent = text;
  el.className = "toast" + (err ? " err" : "");
  el.hidden = false;
  if (!err) toastTimer = setTimeout(() => (el.hidden = true), 5000); // 錯誤訊息保留，直到下一次操作
}

// ---------- 伺服器連線狀態 ----------
// 標籤只顯示一個速度（TCP 連線時間）；顏色反映 TCP 與 UDP 的綜合結果，詳細數據放在提示裡。
//   都通 → 綠；TCP 通但 UDP 不通 → 黃；TCP 不通 → 紅
function stHtml(server) {
  if (!server) return ""; // Default（官方預設）沒有固定位址，不檢查
  const r = reach.get(server);
  if (!r) return `<span class="st st-wait">${esc(t("st.checking"))}</span>`;
  const code = (c) => t("code." + (c || "failed"));
  const tcp = r.ok ? t("st.tip_ok", { ms: r.ms }) : code(r.error);
  const udp = r.udp_ok ? t("st.tip_ok", { ms: r.udp_ms }) : code(r.udp_error);
  const tip = `TCP 21116: ${tcp}\nUDP 21116: ${udp}`;
  let cls, text;
  if (r.ok && r.udp_ok) {
    cls = "st-ok";
    text = `${t("st.online")} · ${r.ms} ms`;
  } else if (r.ok) {
    cls = "st-warn";
    text = `${t("st.partial")} · ${r.ms} ms`;
  } else {
    cls = "st-down";
    text = `${t("st.offline")} · ${code(r.error)}`;
  }
  const aria = `${text}. ${tip.replace("\n", ". ")}`;
  return `<span class="st ${cls}" role="img" title="${esc(tip)}" aria-label="${esc(aria)}">${esc(text)}</span>`;
}

async function checkAll(force) {
  if (checking) return;
  const servers = new Set(state.profiles.map((p) => p.server).filter(Boolean));
  if (state.current) servers.add(state.current); // 目前使用的可能不是已儲存的 profile
  const todo = [...servers].filter((s) => force || !reach.has(s));
  if (!todo.length) return;
  checking = true;
  $("recheck").classList.add("busy");
  try {
    for (const r of await invoke("check_servers", { servers: todo })) reach.set(r.server, r);
  } catch (e) {
    toast(t("toast.check_failed", { err: e }), true);
  } finally {
    checking = false;
    lastCheck = Date.now();
    $("recheck").classList.remove("busy");
    render(); // 舊結果在新結果回來前保留，避免畫面閃爍
  }
}

// ---------- 渲染 ----------
function row(p) {
  const on = p.server === state.current;
  const srv = p.server || t("row.official");
  return `<li class="row ${on ? "on" : ""}" data-n="${esc(p.name)}">
    <button class="star" data-act="fav" aria-pressed="${p.favorite}" aria-label="${esc(t("row.fav_label", { name: p.name }))}" title="${esc(p.favorite ? t("row.fav_remove") : t("row.fav_add"))}">${ICON.star}</button>
    <div class="row-main">
      <div class="name" title="${esc(p.name)}">${esc(p.name)}${p.builtin ? `<span class="badge">${esc(t("row.builtin"))}</span>` : ""}</div>
      <div class="srv" title="${esc(srv)}">${esc(srv)}</div>
    </div>
    ${stHtml(p.server)}
    ${on ? `<span class="pill act">${esc(t("row.in_use"))}</span>` : `<button class="btn act" data-act="use" aria-label="${esc(t("row.switch_to", { name: p.name }))}">${esc(t("row.switch"))}</button>`}
    ${p.builtin ? '<span class="more-ph"></span>' : `<button class="more" data-act="more" aria-haspopup="menu" aria-expanded="false" aria-label="${esc(t("row.more", { name: p.name }))}">${ICON.dots}</button>`}
  </li>`;
}

function group(title, list) {
  return list.length ? `<section><h2 class="group-title">${esc(title)}</h2><ul class="list">${list.map(row).join("")}</ul></section>` : "";
}

function render() {
  const cur = state.profiles.find((p) => p.server === state.current);
  const curSrv = state.current || t("row.official");
  $("cur-info").innerHTML = `
    <span class="dot" aria-hidden="true"></span>
    <span class="label">${esc(t("cur.label"))}</span>
    <span class="cur-name">${esc(cur ? cur.name : t("cur.custom"))}</span>
    <span class="cur-srv" title="${esc(curSrv)}">${esc(curSrv)}</span>
    ${stHtml(state.current)}`;

  const favs = state.profiles.filter((p) => p.favorite);
  const others = state.profiles.filter((p) => !p.favorite);
  $("groups").innerHTML = group(t("grp.fav"), favs) + group(favs.length ? t("grp.other") : t("grp.all"), others);

  $("admin-warn").hidden = !state.needs_admin;
  $("autostart").checked = state.autostart;
  $("lang").value = state.lang_setting;
  $("platform-desc").textContent = { windows: "Windows", macos: "macOS", linux: "Linux" }[state.platform] || state.platform;
  $("version-desc").textContent = "v" + state.version;
  renderUpdate();
}

// ---------- 應用程式內更新：有新版本時在視窗最上方顯示橫幅，由使用者決定要不要更新 ----------
function setUpdateBusy(busy) {
  $("upd-install").disabled = busy;
  $("upd-later").disabled = busy;
}

function renderUpdate() {
  const info = state.update;
  const bar = $("update-bar");
  if (updating) return; // 下載／安裝期間由進度事件控制畫面
  if (!info || info.version === dismissedVersion) {
    bar.hidden = true;
    return;
  }
  bar.hidden = false;
  $("upd-text").textContent = updateError ? t("upd.failed", { err: updateError }) : t("upd.available", { version: info.version, current: info.current });
  $("upd-install").textContent = updateError ? t("upd.retry") : t("upd.install");
  $("upd-progress").hidden = true;
  setUpdateBusy(false);
  const notes = $("upd-notes");
  notes.hidden = !info.notes;
  $("upd-notes-body").textContent = info.notes || "";
}

$("upd-later").onclick = () => {
  if (state.update) dismissedVersion = state.update.version;
  updateError = null;
  renderUpdate();
};

$("upd-install").onclick = async () => {
  if (updating) return;
  updating = true;
  updateError = null;
  setUpdateBusy(true);
  $("upd-text").textContent = t("upd.downloading_unknown");
  $("upd-progress").removeAttribute("value"); // 沒有進度值 = 不確定進度的動畫
  $("upd-progress").hidden = false;
  try {
    await invoke("install_update"); // 成功時程式會重新啟動，不會回來
  } catch (e) {
    updating = false;
    updateError = String(e);
    renderUpdate();
  }
};

// 設定裡的「檢查更新」
$("check-update").onclick = async () => {
  const btn = $("check-update");
  btn.disabled = true;
  $("update-status").textContent = t("upd.checking");
  try {
    const r = await invoke("check_update");
    if (r.state === "available") {
      dismissedVersion = null;
      updateError = null;
      await load(); // 重新取得狀態，橫幅會在關閉設定後出現在最上方
      $("update-status").textContent = t("upd.available", { version: r.info.version, current: r.info.current });
    } else {
      $("update-status").textContent = r.state === "uptodate" ? t("upd.uptodate") : t("upd.unconfigured");
    }
  } catch (e) {
    $("update-status").textContent = "";
    settingsError(e);
  } finally {
    btn.disabled = false;
  }
};

async function load() {
  try {
    state = await invoke("get_state");
  } catch (e) {
    return toast(t("toast.state_failed", { err: e }), true);
  }
  setLang(state.lang);
  applyStaticI18n();
  render();
  checkAll(false); // 只檢查還沒有結果的（新增的伺服器）；不阻塞畫面
}

// ---------- 更多選單 ----------
function closeMenu(restoreFocus) {
  $("menu").hidden = true;
  if (menuBtn) {
    menuBtn.setAttribute("aria-expanded", "false");
    if (restoreFocus) menuBtn.focus();
  }
  menuFor = menuBtn = null;
  armedDelete = false;
  $("menu").querySelector('[data-m="del"]').textContent = t("menu.delete");
}

function openMenu(btn, name) {
  closeMenu(false);
  menuFor = name;
  menuBtn = btn;
  btn.setAttribute("aria-expanded", "true");
  const m = $("menu");
  m.hidden = false;
  const r = btn.getBoundingClientRect();
  m.style.top = Math.max(8, Math.min(r.bottom + 4, window.innerHeight - m.offsetHeight - 8)) + "px";
  m.style.left = Math.max(8, Math.min(r.right - m.offsetWidth, window.innerWidth - m.offsetWidth - 8)) + "px";
  m.querySelector("button").focus();
}

$("menu").addEventListener("click", async (e) => {
  const b = e.target.closest("[data-m]");
  if (!b || !menuFor) return;
  const name = menuFor;
  if (b.dataset.m === "edit") {
    closeMenu(false);
    openDialog(state.profiles.find((x) => x.name === name));
  } else if (b.dataset.m === "del") {
    if (!armedDelete) {
      armedDelete = true; // 兩段式確認：再按一次才真的刪除
      b.textContent = t("menu.confirm_delete");
      return;
    }
    closeMenu(false);
    try {
      await invoke("delete_profile", { name });
      toast(t("toast.deleted", { name }));
    } catch (er) {
      toast(String(er), true);
    }
    load();
  }
});
document.addEventListener("click", (e) => {
  if (!$("menu").hidden && !e.target.closest("#menu") && !e.target.closest('[data-act="more"]')) closeMenu(false);
});
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape" && !$("menu").hidden) closeMenu(true);
});
window.addEventListener("resize", () => closeMenu(false)); // 視窗縮放時選單位置會失準，直接收起

// ---------- 清單操作 ----------
function startSwitch(name) {
  if (busy) return;
  busy = true;
  toast(t("toast.switching", { name }));
  invoke("use_profile", { name });
}

$("groups").addEventListener("click", async (e) => {
  const b = e.target.closest("[data-act]");
  if (!b) return;
  const name = b.closest(".row").dataset.n;
  const p = state.profiles.find((x) => x.name === name);
  if (b.dataset.act === "use") {
    startSwitch(name);
  } else if (b.dataset.act === "fav") {
    try {
      await invoke("set_favorite", { name, favorite: !p.favorite });
    } catch (er) {
      toast(String(er), true);
    }
    load();
  } else if (b.dataset.act === "more") {
    if (menuFor === name) closeMenu(true);
    else openMenu(b, name);
  }
});

// ---------- 新增／編輯對話框 ----------
function dlgError(text) {
  $("dlg-err").textContent = text || "";
  $("dlg-err").hidden = !text;
}

function openDialog(p) {
  p = p || {};
  const existing = !!p.name && state.profiles.some((x) => x.name === p.name);
  $("dlg-title").textContent = existing ? t("dlg.title_edit", { name: p.name }) : t("dlg.title_add");
  FIELDS.forEach((k) => ($("f-" + k).value = p[k] || ""));
  editingOld = existing ? p.name : null;
  $("raw").value = "";
  dlgError("");
  $("dlg").showModal();
  $("f-name").focus();
  $("f-name").select();
}

async function parseRaw() {
  const text = $("raw").value;
  if (!text.trim()) return dlgError("");
  try {
    const p = await invoke("parse_text", { text });
    ["server", "relay", "api", "key"].forEach((k) => ($("f-" + k).value = p[k] || ""));
    if (!$("f-name").value) $("f-name").value = p.name; // 只在名稱還空著時，才用主機位址當預設名稱
    dlgError("");
  } catch (e) {
    dlgError(String(e));
  }
}

$("raw").addEventListener("input", parseRaw);
$("add").onclick = () => openDialog();
$("cancel").onclick = () => $("dlg").close();

$("form").addEventListener("submit", async (e) => {
  e.preventDefault(); // 失敗時要留在對話框內顯示錯誤
  if (!$("f-name").value.trim()) {
    dlgError(t("dlg.name_required"));
    return $("f-name").focus();
  }
  const apply = e.submitter && e.submitter.value === "apply";
  const profile = {};
  FIELDS.forEach((k) => (profile[k] = $("f-" + k).value.trim()));
  try {
    await invoke("save_profile", { profile, oldName: editingOld });
  } catch (er) {
    return dlgError(String(er));
  }
  $("dlg").close();
  await load();
  if (apply) startSwitch(profile.name);
  else toast(t("toast.saved"));
});

// ---------- 設定 ----------
$("open-settings").onclick = () => {
  $("set-err").hidden = true;
  $("settings").showModal();
};
$("close-settings").onclick = () => $("settings").close();

function settingsError(er) {
  $("set-err").textContent = String(er); // 對話框蓋住 toast，錯誤要顯示在對話框內
  $("set-err").hidden = false;
}

$("autostart").addEventListener("change", async (e) => {
  try {
    await invoke("set_autostart", { enabled: e.target.checked });
    $("set-err").hidden = true;
  } catch (er) {
    settingsError(er);
  }
  load();
});

$("lang").addEventListener("change", async (e) => {
  try {
    await invoke("set_language", { lang: e.target.value });
    $("set-err").hidden = true;
  } catch (er) {
    settingsError(er);
  }
  load(); // 取得新的有效語言並重新套用整個畫面
});

// ---------- 狀態重新檢查：手動、視窗取得焦點、視窗可見時每 30 秒 ----------
$("recheck").onclick = () => checkAll(true);
window.addEventListener("focus", () => {
  if (Date.now() - lastCheck > 10000) checkAll(true);
});
setInterval(() => {
  if (document.visibilityState === "visible") checkAll(true);
}, 30000);

// ---------- 事件 ----------
listen("toast", (ev) => {
  busy = false;
  toast(ev.payload.msg, !ev.payload.ok);
});
listen("changed", load);
listen("update-available", load); // 背景檢查找到新版本：重新取得狀態，橫幅會自動出現
listen("update-progress", (ev) => {
  const { downloaded, total } = ev.payload;
  const bar = $("upd-progress");
  if (total) {
    const pct = Math.min(100, Math.floor((downloaded / total) * 100));
    bar.value = pct;
    $("upd-text").textContent = t("upd.downloading", { pct });
  }
});
listen("update-installing", () => {
  $("upd-progress").removeAttribute("value");
  $("upd-text").textContent = t("upd.installing");
});
load();
