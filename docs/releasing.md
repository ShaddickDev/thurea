# Updates, releases and the repository layout

## Updates

Every Thurea updates itself from this repository's releases, checking every hour (Windows:
the "Thurea Update" task; Linux: `thurea-update.timer`; Android: Thurea's background job). A release is installed only if

1. `thurea-manifest.json` is signed with Thurea's release key — the public half ships with every
   install (`release/release-key.pub.pem`) — so a release that wasn't made with the key is ignored,
   even if someone could upload to this repository, and
2. the downloaded file matches the SHA-256 in that signed manifest, and
3. it is newer than what's installed. (Android additionally requires the same APK signing key.)

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

The signing keys never leave the computer that pushes: nothing secret is stored on GitHub.
`release/auto-release.sh --dry-run`
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
