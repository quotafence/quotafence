# Install, Upgrade, and Remove the Beta

QuotaFence beta artifacts are produced by GitHub Actions. macOS artifacts are
universal binaries; Windows preview artifacts target x64. Production signing is
not yet available, so verify the downloaded files before bypassing an operating
system warning.

## Verify the artifact

Extract the complete artifact so `SHA256SUMS.txt` is next to the files it
describes.

On macOS:

```bash
shasum -a 256 -c SHA256SUMS.txt
```

On Windows PowerShell:

```powershell
Get-Content .\SHA256SUMS.txt | ForEach-Object {
  $expected, $name = $_ -split "  ", 2
  $actual = (Get-FileHash -Algorithm SHA256 ".\$name").Hash.ToLower()
  if ($actual -ne $expected) { throw "Checksum mismatch: $name" }
  Write-Host "OK  $name"
}
```

Do not continue when a checksum is missing or different. `BUILD-INFO.txt`
records the commit, workflow run, and signing mode that produced the artifact.

## macOS desktop app

Open the DMG and drag QuotaFence into Applications. The ad-hoc beta is not
notarized, so Gatekeeper can warn on first launch even when its checksum is
correct.

After verifying the checksum, use Finder to Control-click **QuotaFence.app**,
choose **Open**, and confirm **Open**. If macOS still blocks it, open **System
Settings → Privacy & Security**, find the QuotaFence message, choose **Open
Anyway**, and authenticate. Do not disable Gatekeeper globally and do not run a
blanket `xattr` command against Applications.

## Windows desktop app

Run `QuotaFence_<version>_x64-setup.exe`. The preview is unsigned, so Microsoft
Defender SmartScreen may show **Windows protected your PC**. After verifying the
checksum and `BUILD-INFO.txt`, choose **More info → Run anyway**. Do not add a
global Defender exclusion.

WebView2 is normally present on supported Windows versions. If the app opens to
an empty window, install the current Microsoft Edge WebView2 Runtime and retry.

## CLI

The artifact includes install and uninstall scripts. See the [CLI guide](cli.md)
for the exact macOS and Windows commands. The CLI and desktop app share one
local database.

## Upgrade

1. Exit QuotaFence and finish managed CLI sessions.
2. Download and verify the newer complete artifact.
3. Replace the macOS app or run the newer Windows installer.
4. Run the newer CLI install script if the CLI is installed.
5. Open QuotaFence, Sync each provider, and verify quota/reset values.

Database migrations run when the newer app or CLI opens the existing database.
Downgrades are not guaranteed; back up the database before testing an older
build.

## Remove

Disable and uninstall Codex/Claude protection from QuotaFence Settings before
removing the app. This preserves unrelated provider hooks.

- macOS: quit QuotaFence and move it from Applications to Trash.
- Windows: uninstall QuotaFence from **Settings → Apps → Installed apps**.
- CLI: run the platform `uninstall-cli` script from the extracted artifact.

Removing the desktop app or CLI does not intentionally erase the quota ledger.
The database remains at:

- macOS: `~/Library/Application Support/com.buisonanh.quotafence/quotafence.sqlite3`
- Windows: `%APPDATA%\com.buisonanh.quotafence\quotafence.sqlite3`

To remove all local QuotaFence state, first quit the app and managed sessions,
back up anything needed, then delete the entire
`com.buisonanh.quotafence` directory. This cannot be undone. QuotaFence does not
delete Codex or Claude credentials, transcripts, or source folders.

## Report an installation problem

Use the repository's **Installation or upgrade problem** issue form. Include
the operating system, artifact name, `BUILD-INFO.txt`, and the failing step.
Never attach provider credentials, the QuotaFence database, prompts,
transcripts, or source code.

