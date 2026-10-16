# rdswitch

[English](README.md) · [繁體中文](README.zh-TW.md)

A small system-tray app for switching between [RustDesk](https://rustdesk.com) servers (ID / relay / API server and public key) with one click. Built with [Tauri 2](https://tauri.app) (Rust core + the OS WebView), targeting Windows, macOS and Linux.

> **Unofficial.** This project is not affiliated with, endorsed by, or sponsored by RustDesk.
> Status: only verified on Windows so far — see [Known limitations](#known-limitations).

## Features

- **One-click switching** from the tray menu or the main window. Each profile holds an ID server, relay server, API server and public key; empty fields mean "RustDesk default".
- **Default profile**: built in and read-only; it represents RustDesk's official defaults.
- **Favorites**: star profiles to pin them to the top of the list and to the top level of the tray menu (the rest go into a submenu).
- **Server status**: every row shows whether the ID server is reachable (TCP *and* UDP on port 21116) with its latency; hover for the details.
- **Paste to fill**: paste a RustDesk config string, JSON, or `ID server: … / Key: …` text into the add/edit dialog and the fields are filled in for you.
- **Languages**: English, 繁體中文, 简体中文, 日本語 — follows the system language by default and can be changed in Settings. Adding another language is a matter of adding one dictionary per side (see [Architecture](#architecture)).
- **Responsive layout** and a light/dark theme that follows the system.
- **Start at login** (optional).
- **In-app updates** (optional, see [Updates](#updates)).

Tray menu (right-click; left-click opens the window):

```
  Current: Office                        ← greyed out, not clickable
────────────────────────
✓ Office                                 ← favorites are listed directly (✓ = in use)
  Home
  More ▸                                 ← everything else; with no favorites, all profiles are listed directly
────────────────────────
  Open window
✓ Start at login
────────────────────────
  Quit
```

## How switching takes effect

If RustDesk is installed as a **system service**, the running service keeps its settings in memory, so **editing the config file alone does not take effect**. The app therefore does this:

| RustDesk installation | What the app does | Privileges |
|---|---|---|
| Installed as a system service | Calls `rustdesk --option <key> <value>` so the running service is notified, then reads the value back to verify | Administrator / root |
| Portable / no service | Closes the RustDesk GUI → backs up → atomically rewrites `RustDesk2.toml` → restarts the GUI | Normal user |

The Windows build carries a `requireAdministrator` manifest, so it asks for UAC at launch. For the same reason, "Start at login" uses a scheduled task with highest privileges (a plain `Run` registry entry cannot start an elevated program at logon).

The settings that are switched: `custom-rendezvous-server`, `relay-server`, `api-server` and `key`. A backup of the previous config is written to the app data folder before every change.

## Server status

For each profile's ID server the app tries, in parallel:

- **TCP**: opens a connection to port 21116 (default) and measures the time.
- **UDP**: UDP has no connections, so the app sends one minimal RustDesk protocol request (`RegisterPeer` with the throw-away ID `rdswitch-probe`) and validates the reply. The server answers with `RegisterPeerResponse` and does not store the fake ID. The packet layout follows RustDesk's published `rendezvous.proto` and was verified against real `hbbs` servers; no RustDesk source code is included in this project.

The label shows one latency (the TCP connect time); the dot color combines both results — green: both work, amber: only TCP works, red: TCP failed. Hover for both measurements. Checks run when profiles are added, when the window gains focus, every 30 seconds while it is visible, and on demand (refresh button). The default profile has no fixed address and is not checked.

Only the ID server is checked; the relay (21117), NAT-test (21115) and API ports are not.

## Updates

The app can check for a newer version and install it in place — no need to download an installer from GitHub by hand.

- A few seconds after start and then every 6 hours, the app asks the release feed whether a newer version exists. If so, a banner appears at the top of the window showing the new version and its release notes. **Nothing is installed without your click**: choose *Update now* (the update is downloaded, its signature verified, installed, and the app restarts) or *Later* (dismisses the banner until the next launch).
- Every update is signed. The app only installs updates whose signature matches the public key embedded in the app; an update without a valid signature is rejected. The release feed itself (`latest.json`) is *not* signed, so the app also requires the signature to carry the version it was signed for (`requireSignedVersion`) and rejects the update if that differs from the version the feed announces. This blocks a forged feed from pairing a high version number with an older, genuinely signed (but outdated) build.
- Checks only contact the configured release feed (GitHub Releases) over HTTPS. There is no telemetry.
- The updater only runs when both `plugins.updater.pubkey` and `plugins.updater.endpoints` are configured; this repository ships with both, so official builds check for updates. If either is missing (for example in a fork that has not set up its own key), the updater stays off and Settings shows "no update source is configured". See [Releasing](#releasing-and-enabling-updates).

## Privacy

- No telemetry, analytics or accounts.
- Profiles (including public keys) are stored in plain text in the app data folder (permissions `0600` on macOS/Linux). They never leave your computer.
- The only network traffic is: the TCP/UDP status probes to *the servers you configured*, and — if the updater is configured — the update check against the release feed.

## Data locations

| OS | Folder |
|---|---|
| Windows | `%APPDATA%\rdswitch` |
| macOS | `~/Library/Application Support/rdswitch` |
| Linux | `~/.config/rdswitch` |

Files: `profiles.json`, `favorites.json`, `settings.json` (language), `backups/`.

## Build

Requirements: Node 18+, [rustup](https://rustup.rs) (the exact Rust version, with `rustfmt` and `clippy`, is pinned in `rust-toolchain.toml` and installed for you with `rustup install`), and the [Tauri prerequisites](https://tauri.app/start/prerequisites/) for your OS (Linux: `libwebkit2gtk-4.1-dev libayatana-appindicator3-dev`).

```
npm install
npm run tauri dev                 # development (on Windows, run from an elevated terminal)
cargo test --locked --manifest-path src-tauri/Cargo.toml
node scripts/check-i18n.mjs      # front-end translation consistency
node scripts/check-version.mjs   # the five version numbers must agree
npm run tauri build               # installers: nsis, dmg, deb/rpm/AppImage
```

`cargo test` never touches a real RustDesk installation. The end-to-end checks below **temporarily change your real RustDesk settings** and need administrator/root:

```
RDS_E2E_PROFILE=<name of an existing profile> cargo test -- --ignored --nocapture --test-threads=1
```

> **If you distribute your own build**, rewrite local paths so they are not embedded in the binary (they appear in panic messages):
> `RUSTFLAGS="--remap-path-prefix=$PWD=/build --remap-path-prefix=$HOME/.cargo=/cargo --remap-path-prefix=$HOME/.rustup=/rustup"`.
> The release workflow already does this.

## Releasing and enabling updates

Installers for all three platforms are built by `.github/workflows/release.yml` when a `v*` tag is pushed. To turn on in-app updates the maintainer needs to:

1. Generate a signing key pair (once), **outside the repository**, from a normal terminal (it asks for a password): `npx tauri signer generate -w ~/.tauri/rdswitch.key`. **Never commit the private key** (`*.key` is git-ignored as a safety net). If it is lost, existing installs can no longer be updated.
2. Put the **public** key (`rdswitch.key.pub`) into `src-tauri/tauri.conf.json` → `plugins.updater.pubkey`. This repository already contains the public key of the official release key; a fork must use its own.
3. `plugins.updater.endpoints` already points to `https://github.com/shen-labs/rdswitch/releases/latest/download/latest.json`. If you fork the project, change it to your own repository.
4. Add the repository secrets `TAURI_SIGNING_PRIVATE_KEY` (the key's contents) and, if you set one, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.
5. Bump the version (`package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`), push a `v*` tag, and **publish** the draft release the workflow creates. Clients only see published, non-draft, non-prerelease releases.

Before announcing it, test the whole chain once on a real release: install version *N*, publish *N+1*, and confirm the banner appears and the update installs. (The automated tests below cover the signature checks, **not** the real feed or the installer/restart step.)

**Signature tests (Windows x64).** `src-tauri/tests/updater_e2e.rs` runs the real updater plugin against a local fake feed and a real signed installer, without installing anything. It checks that a genuine update verifies and downloads byte-for-byte, and that a tampered file, a wrong key, and a forged higher version number are all rejected. It is skipped unless these variables are set:

```
npx tauri signer generate --ci -w /tmp/test.key            # throw-away key, not your release key
npx tauri signer generate --ci -w /tmp/other.key           # an unrelated key
# build a signed installer with the throw-away key (TAURI_SIGNING_PRIVATE_KEY=<key contents>, plus
# --config src-tauri/tauri.release.conf.json and a config that sets plugins.updater.pubkey to /tmp/test.key.pub)
RDS_UPDATER_TEST_EXE=<path to the signed *-setup.exe> \
RDS_UPDATER_TEST_PUB=/tmp/test.key.pub \
RDS_UPDATER_TEST_OTHER_PUB=/tmp/other.key.pub \
cargo test --manifest-path src-tauri/Cargo.toml --test updater_e2e -- --nocapture
```

Optionally set `RDS_UPDATER_TEST_UNVERSIONED_EXE` to a copy of the installer signed with `npx tauri signer sign` (which records no version) to also show what `requireSignedVersion` protects against.

## Architecture

```
src/                  Front end (plain HTML/CSS/JS, no bundler): top bar + status bar + list
  i18n.js             Front-end dictionaries (UI text)
src-tauri/src/
  platform.rs         Platform adapter: paths, privileges, RustDesk lookup & service detection,
                      --option, start-at-login, system language (the only place that checks the OS)
  tomledit.rs         Edits only [options] in RustDesk2.toml; everything else is kept as is
  parse.rs            Parser for the "paste config text" box (config string / JSON / labelled text)
  probe.rs            Server status: TCP connect + UDP protocol probe (DNS lookups are time-boxed)
  profiles.rs         Profiles and favorites: storage, Default protection, applying a profile
  i18n.rs             Back-end dictionaries (tray menu, error messages)
  settings.rs         User settings (language)
  lib.rs              Tray menu, Tauri commands, single instance, close-to-tray
src-tauri/windows-admin.manifest   Run as administrator on Windows
```

To add a language: add it to `Lang`/`ALL`/`TABLE` in `i18n.rs`, to `LANGS`/`DICT` in `src/i18n.js`, and an `<option>` in the language select. Both dictionaries are covered by consistency checks (every key in every language, same placeholders).

## Relationship to RustDesk

- This is an independent, unofficial tool. "RustDesk" is the name of a project and product owned by others; this project does not use RustDesk's logo or artwork. It is deliberately named `rdswitch` rather than after RustDesk, and the name "RustDesk" appears only to say what the tool works with. I could not find a published RustDesk trademark policy, so the absence of a rule is not a grant of permission; if you are unsure, ask the RustDesk team.
- RustDesk's client is released under the AGPL-3.0. This project contains none of its source code; it only edits RustDesk's configuration, calls its command line (`rustdesk --option`), and sends one small packet to servers you configured. This is not legal advice.
- `rustdesk --option <key> <value>` is what makes a switch take effect when RustDesk runs as a service, but the public client-configuration documentation only describes `rustdesk --config <string>`; I could not find `--option` documented. It works with the versions tested so far, but it is not a documented interface and may change.

## Known limitations

- Only verified on Windows. The macOS/Linux config paths, `--option`, service detection and the way RustDesk is closed/started have not been tested on real machines. On those systems, switching with an installed service needs root, so start the app with `sudo`.
- Installers are not code-signed (Windows Authenticode / macOS notarization), so the OS will show a security warning.
- Every switch needs administrator privileges when RustDesk runs as a service (on Windows: the UAC prompt at launch); there is no privileged helper yet.
- UDP/relay/API reachability is only partly covered; see [Server status](#server-status).

## License

[MIT](LICENSE) © 2026 SHEN. Third-party dependencies (Tauri and others) keep their own licenses; if you distribute binaries, include their license notices (for example generated with `cargo about`).
