# Windows installer verification

BALD uses Tauri 2 and NSIS in currentUser mode. The production identifier remains
`app.bald.desktop`, product name `Bald`, and binary name `bald.exe`.

## Preserved successful baseline, 2026-10-10

The user confirmed that Windows search and its BALD icon currently work. Before
changing the installer, the live shortcut and settings were copied to
`target/shortcut-baseline/20261010-081234`. The original shortcut was only read.
`baseline.json` records its hash, timestamp, attributes and ACL; `shell-link.json`
records its actual Shell Link structure. These are local diagnostics, not release artifacts.

| Field | Current successful shortcut | Supplied recipe |
| --- | --- | --- |
| Target | `%LOCALAPPDATA%\Bald\bald.exe` | Same |
| Working directory | `%LOCALAPPDATA%\Bald` | Same |
| Icon | `%LOCALAPPDATA%\Bald\icons\icon.ico,0` | `bald.exe,0` |
| Explicit AppUserModelID | Absent | Absent |

The shortcut SHA-256 is `6864B5733FA86CB4312DA37C32604A2CAB0887BBEE4B8B83798DB6391DA38419`.
Arguments and description are empty; icon index is 0, ShowCommand is 1, HotKey is
0, and LinkFlags is `0x000040DB`. StringData contains relative target, working
directory and explicit icon path. ExtraData contains IconEnvironment (`A0000007`),
KnownFolder (`A000000B`), Tracker (`A0000003`) and PropertyStore (`A0000009`) blocks,
followed by a valid terminator. A PropertyStore block alone does not imply that
an explicit AppUserModelID exists. Get-StartApps reports Bald with its EXE path
as the application identity.

## Installer behavior

`tools/windows/installer.nsi` is the Tauri NSIS template based on CLI 2.12.1.
`installer-provenance.json` pins its upstream commit and original hash;
`LICENSE.tauri` carries its MIT notice. `tauri.conf.json` selects this template.
The former `installer-hooks.nsh` is no longer configured.

- New Start menu and desktop shortcuts explicitly target `$INSTDIR\bald.exe`,
  use `$INSTDIR` as the working directory, and reference
  `$INSTDIR\icons\icon.ico,0`, matching the observed successful BALD shortcut.
- The installed ICO and all nine embedded EXE icon images must match the source
  icon. Shell-rendered shortcut icons must match the successful WScript recipe
  using `bald.exe,0`, and the live successful BALD shortcut when available.
- Existing shortcuts return before any save or migration. Bytes, timestamps,
  attributes, ACL, arguments and property store are preserved.
- The production bundle identifier remains `app.bald.desktop`. New shortcuts keep
  Tauri's stable AppUserModelID. Existing shortcuts are never assigned or stripped
  of an ID, including the successful user-created shortcut with no explicit ID.
  AppUserModelID has not been established as the cause of the previous UI issue.
- Updates restore a missing Start menu shortcut unless /NS was selected. That
  preference is saved for subsequent /UPDATE runs. Ordinary installation without
  /NS enables shortcut creation again.
- Upgrade maintenance passes /UPDATE to the old NSIS uninstaller, preserving
  shortcuts, startup preferences and app data during replacement.
- Normal removal uses upstream target checks to remove shortcuts belonging to
  this installation. App data deletion remains an explicit user choice.

## Automated release verification

Use PowerShell 7 and 7-Zip on Windows:

```powershell
npm run build
npm run verify:installer
```

The verifier extracts the real release installer and checks its executable,
version, installed ICO and all nine embedded icon images. It exercises the NSIS
lifecycle using a temporary installation identity and the extracted release
payload. This prevents production BALD from being uninstalled or launched during
removal tests. These are installation tests, not Windows search experiments.

Required checks cover fresh install, target, working directory, explicit ICO
reference, Shell Link structure and rendered icon, uninstall registration,
existing Start/desktop links on update and reinstall, user-created links without
an explicit ID, custom arguments and description, missing link recovery, version
upgrade and maintenance upgrade, upgrade uninstall/reinstall, removal, reinstall,
saved /NS, startup preference and settings preservation.

When the live BALD shortcut is present, checks also compare its relative icon
location, target binary name, icon index, ShowCommand, HotKey, LinkFlags and
rendered icon against newly installed shortcuts. Paths are normalized to each
installation directory. Different AppUserModelID presence is checked separately:
new shortcuts retain Tauri's ID, existing links preserve their own metadata.
Production shortcut, executable, ICO, uninstaller and settings must retain hashes,
timestamps, attributes and ACL. Previously absent protected files remain absent.

The old-version fixture uses the current payload to test installer behavior,
not an older app's runtime. Temporary test installations, shortcuts, settings and
registry entries are removed. Existing build/test artifacts remain under target.

GitHub Actions requires this automated lifecycle verification before publishing
its staged release and update manifest. The verifier does not call Get-StartApps,
query search databases, retain test apps for UI experiments, or require search
results for temporary test identities. Their search visibility is not a release
condition.

## Actual BALD manual UI verification

The user has already confirmed that the current real BALD shortcut appears in
Windows search with its correct icon and launches correctly. That successful
configuration is the baseline, and the installed production shortcut is preserved.

After installing or updating the actual BALD release, verify separately:

1. Searching for BALD displays the installed BALD application.
2. The search result and Start menu display the BALD icon.
3. Selecting the result launches the installed application normally.

Native Windows search UI cannot be checked directly in this Codex environment.
Only these UI checks require user confirmation; temporary test app search results
do not block the build or release. No Windows search database or cache is reset,
modified or repaired by the installer or verifier.

References: [Tauri NSIS templates and hooks](https://v2.tauri.app/distribute/windows-installer/),
[AppUserModelIDs](https://learn.microsoft.com/en-us/windows/win32/shell/appids),
[Shell Link format](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-shllink/).