# Updates, releases and the repository layout

## Updates

Every Thurea updates itself from this repository's releases, checking every hour (Windows:
the "Thurea Update" task; Linux: `thurea-update.timer`; Android: Thurea's background job). A release is installed only if

1. `thurea-manifest.json` is signed with Thurea's release key — the public half ships with every
   install (`release/release-key.pub.pem`) — so a release that wasn't made with the key is ignored,
   even if someone could upload to this repository, and
2. the downloaded file matches the SHA-256 in that signed manifest, and
3. it is newer than what's installed. (Android additionally requires the same APK signing key.)

What each app gets, with no reinstall:

| App | How it updates |
|---|---|
| Windows (zip or MSIX) | the "Thurea Update" task installs the signed `thurea-windows.zip` into Program Files\Thurea; an MSIX install hands over to the same copy, so it updates the same way |
| Linux: Arch / Omarchy, Debian / Ubuntu, Fedora | `thurea-update.timer` installs the signed package with pacman, apt or dnf |
| Omarea (Omarchy) | ships in the Linux package; a `omarchy plugin add` checkout of this repository is swapped for the package's copy once Thurea is installed, so it updates with it |
| Linux tray (`thurea-tray`) | restarts itself into the new version once the package is updated |
| Windows tray | stopped and restarted by the update |
| Family control centre | part of `thurea`; a control centre still open from an older version isn't reused (it closes when idle) |
| Browser extension, Chromium family | force-installed from this computer's update server (`127.0.0.1:8137/updates.xml`), which the desktop update refreshes |
| Browser extension, Firefox family | force-installed by policy from a versioned `install_url`, so Firefox reinstalls it after each desktop update. Firefox and Zen only accept it once it is signed by Mozilla (`AMO_JWT_*` secrets); until then only builds that allow unsigned add-ons take it |
| Android | Thurea's background job. As device owner it installs silently; otherwise Android 12+ installs silently too (Thurea updating itself), and older Android or a phone that still asks gets a "Thurea update ready" notification to tap. An update installs over the installed Thurea (same package and signing key) and never signs accounts out; `android/phone-setup.sh` on a phone where Thurea is already device owner or device admin updates it the same way. Only first becoming device owner needs accounts signed out once. Settings → Updates shows the last check and checks at once when tapped. If a release is signed with a different Android key than the installed APK, Thurea says so there instead of failing silently; installing it once with `thurea update-phone` fixes it |

To check by hand, without waiting for the hour:

- Windows (as Administrator): `powershell -ExecutionPolicy Bypass -File "C:\Program Files\Thurea\Thurea.ps1" -Update` (add `-CheckOnly` to only look). The result is in Thurea's log.
- Linux: `sudo /usr/lib/thurea/thurea-update` (or `--check`), or `journalctl -u thurea-update` for the hourly runs.
- Android: Settings → Updates → Check for updates, or over adb `adb shell am start -n com.thurea.guard/.Remote --ez check_update true`.

## Release from GitHub (any computer)

`.github/workflows/release.yml` builds every package on GitHub, signs them and publishes the
release, so a release needs only a browser signed in to the account:

1. Put the new version in `release/VERSION` (and a `## X.Y.Z` section in `CHANGELOG.md`, which
   becomes the release notes) and merge it to `master`.
2. **Actions → Release → Run workflow** on `master` (or `gh workflow run release.yml`, or push a
   tag `vX.Y.Z` that matches `release/VERSION`).
3. The run waits for approval of the **release** environment: **Review deployments → Approve**.
   It then builds in an Arch Linux container, signs with the keys from that environment's
   secrets, publishes `vX.Y.Z` as the latest release and tags the commit. A version that is
   already released is refused.

The keys are secrets of the `release` environment (Settings → Environments → release), never
repository secrets. That environment requires ShaddickDev's approval for every run and only
accepts runs from `master` and `v*` tags, so a workflow changed on another branch or in a pull
request can't read them. Pull requests that change the release scripts run **Test build**: the
same build with throwaway keys made in the run, with no secrets and nothing published.

| Secret | From | Needed |
|---|---|---|
| `THUREA_RELEASE_KEY` | `~/.config/thurea/release-key.pem` (the whole PEM text) | yes |
| `THUREA_EXTENSION_KEY` | `~/.config/thurea/extension-key.pem` | yes |
| `THUREA_ANDROID_KEYSTORE` | `~/.config/thurea/android-release.jks`, base64 | yes |
| `THUREA_ANDROID_KEYSTORE_PASS` | `~/.config/thurea/android-keystore-pass` | yes |
| `AMO_JWT_ISSUER`, `AMO_JWT_SECRET` | addons.mozilla.org → Developer Hub → API keys | for a Firefox add-on that Firefox and Zen install |
| `THUREA_WINDOWS_PFX`, `THUREA_WINDOWS_PFX_PASS` | a code-signing `.pfx` (base64) and its password | to Authenticode-sign the Windows files |

Set the repository variable `THUREA_REQUIRE_SIGNED` to `1` to make a run fail rather than publish
an unsigned Windows or Firefox build. The repository variable `THUREA_MSIX_PUBLISHER` sets the
MSIX installer's publisher; it must be the code-signing certificate's subject exactly, and with
`THUREA_WINDOWS_PFX` it defaults to that (see [docs/windows.md](windows.md), "The MSIX installer").

## Push to release

Once a Thurea is installed, updates arrive by themselves, straight from git:

1. **Push** the main branch. `release/hooks/pre-push` (turned on with `release/install-hooks.sh`)
   waits until the push has landed, then runs `release/auto-release.sh` in the background.
2. **auto-release.sh** builds that exact commit in a clean checkout, never from uncommitted
   files. It numbers it (the last release + 0.0.1, or a higher `release/VERSION` in the commit),
   signs the manifest, publishes it as the latest GitHub release and tags the commit. A push that
   only changes documentation or tests isn't released. Logs: `~/.local/state/thurea-release/`.
3. **Every installed Thurea checks hourly** (Windows task, Linux `thurea-update.timer`,
   Android's job) and installs the new signed release by itself, so a push reaches every device
   within about an hour.

The pre-push hook checks outgoing commits for private feature code, and the publisher checks the
contents of the signed release archives, including the copies of the extension bundled with desktop
installers. `THUREA_NO_RELEASE=1` skips the automatic release and still checks outgoing commits.

With this route the signing keys stay on the computer that pushes. `release/auto-release.sh --dry-run`
builds and signs without publishing.

## Building a release

```
release/build.sh 1.0.1        # builds every platform into dist/ and signs the manifest
release/publish.sh            # uploads dist/ as GitHub release v1.0.1 (latest)
```

Building needs, besides Rust, the Android SDK and Chromium: `makepkg` (Arch package),
`dpkg-deb` and `rpmbuild` for the Debian / Fedora packages — on Arch: `sudo pacman -S dpkg rpm-tools`.

The signing keys are never in this repository: `~/.config/thurea/release-key.pem` (release
manifest), `~/.config/thurea/extension-key.pem` (extension ID), and the Android keystore used by
`android/build.sh`.

## Layout

```
extension/   the Thurea browser extension (and host/, its optional native helper)
linux/       Thurea for Linux: stage.sh (one file layout) → PKGBUILD (Arch), debian/ (.deb), rpm/ (.rpm);
             thurea-apply, systemd units, pacman hooks, self-update, Omarea (the Omarchy plugin and `omarea`)
windows/     Thurea for Windows (thurea/Thurea.ps1, tray app) and the Windows 11 ISO build
android/     Thurea for Android
crates/      Rust: thurea-core (shared by Android and desktop), thurea-agent (`thurea`: phone setup,
             verses, status, household mesh), thurea-tray (Linux tray outside Omarchy 4)
policy/      verses.json (World English Bible), blocklist.txt, update-lists.sh
bend/        audit.bend: bulk blocklist audits on the Bend/HVM parallel runtime (`thurea audit --engine bend`);
             tests/ and test.sh: Bend property tests for bedtime windows and the audit (`bend/test.sh`)
release/     build.sh, publish.sh, release public key, VERSION
lab/         isolated test lab (VMs / emulator, no internet)
```
