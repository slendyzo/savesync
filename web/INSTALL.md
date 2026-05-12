# Installing SaveSync

SaveSync ships as a portable binary on every release. No installer, no
admin rights, no system-level state — drop the binary anywhere you can
execute things.

- [Windows](#windows)
- [Linux](#linux)
- [First-run setup](#first-run-setup)
- [Troubleshooting](#troubleshooting)

---

## Windows

1. Open <https://github.com/slendyzo/savesync/releases/latest>
2. Download the file ending in **`-setup.exe`** (NSIS installer) or
   **`.msi`** (Windows Installer)
3. Run it. Until our SignPath OSS code-signing approval lands, Windows
   shows a SmartScreen warning ("Microsoft Defender SmartScreen
   prevented an unrecognized app from starting"). Click **More info →
   Run anyway**.
4. SaveSync launches and shows the onboarding wizard.

The installer adds SaveSync to your Start menu. To remove it later,
use Settings → Apps → SaveSync → Uninstall.

## Linux

1. Open <https://github.com/slendyzo/savesync/releases/latest>
2. Download the **`.AppImage`** file
3. Make it executable and run it:

   ```bash
   chmod +x SaveSync-*.AppImage
   ./SaveSync-*.AppImage
   ```

4. SaveSync launches and shows the onboarding wizard.

Tested on:
- Ubuntu 22.04+ / Debian 12+
- Fedora 39+
- Bazzite / SteamOS (ROG Ally, Steam Deck)
- Arch Linux

If you see "webkit2gtk-4.1 not found", install the matching package
for your distro — Tauri uses it for the embedded webview.

---

## First-run setup

The wizard walks you through three steps:

1. **Connect GitHub** — click "Continue with GitHub", type the 8-char
   code in your browser, approve, done. Or use a Personal Access
   Token with `repo` scope.
2. **Pick or create a repo** — SaveSync auto-creates a private
   `savesync-data` repo on your account by default. Or paste an
   existing repo URL.
3. **Add your games** — SaveSync scans your Steam library and
   pre-checks every game with a known save path.

Total time: about 60 seconds.

---

## Troubleshooting

### Windows shows "Unknown publisher" / SmartScreen warning

Expected until our SignPath code-signing approval lands.
**More info → Run anyway** to continue. Once signed, the warning
disappears.

### "git-lfs not found" when committing a large save

SaveSync bundles `git-lfs` in the release binary, but if you're
running a dev build, install it via your package manager:

- Windows: `winget install GitHub.GitLFS`
- Linux: `sudo apt install git-lfs` / `sudo dnf install git-lfs`
- macOS: `brew install git-lfs`

### Conflict resolved but I want the *other* version back

When SaveSync resolves a conflict, the loser's state is preserved on a
backup branch named `backup/<game>/<machine>-<timestamp>`. To restore
it:

```bash
cd ~/.local/share/savesync/repo   # or %APPDATA%\savesync\repo on Windows
git checkout backup/elden-ring/ROG-Ally-2026-05-12-2031
# copy the game's folder contents back to your save location
```

A "Restore from backup" button is coming in a future iteration.

### My save folder isn't in the Ludusavi manifest

The bundled database covers ~13,000 games but newer or obscure
releases may not be there yet. From the main view's "Add Game"
(coming in a follow-up), enter the save folder manually. For now you
can edit `~/.config/savesync/config.json` directly to add a game with
an arbitrary save path.

### Can I move SaveSync to another folder?

Yes — it's portable. Move the binary anywhere. Your config and the
synced repo stay in their OS-standard locations
(`~/.config/savesync/` on Linux, `%APPDATA%\savesync\` on Windows).

### How do I uninstall completely?

1. Open SaveSync → Settings → **Disconnect this machine**. This wipes
   the per-machine config and clears credentials from your keychain.
2. Delete the SaveSync binary (Windows: use Add/Remove Programs;
   Linux: just delete the AppImage).
3. Your data repo on GitHub stays — it's yours. Delete it manually if
   you want to wipe everything.

---

Issues? Questions? <https://github.com/slendyzo/savesync/issues>
