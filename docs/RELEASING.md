# Releasing Devti

Every installer is built by GitHub Actions from a version tag — nobody's laptop is in the
chain, and the checksums are published next to the files.

## 1. Prepare

1. CI is green on the commit you want to release (every job, including iOS and the
   performance run).
2. The version is set in `Cargo.toml` (workspace), `desktop/package.json`,
   `desktop/src-tauri/tauri.conf.json`, `android/app/build.gradle.kts` (`versionName`, and
   a higher `versionCode`) and `ios/project.yml` (`MARKETING_VERSION`).
3. `CHANGELOG.md` has a section for the version and `docs/release-notes/v<version>.md` exists
   (the release page shows it).

## 2. Sign Android with your own key (recommended)

Without a key, the release APK is signed with a throwaway debug key, and an APK signed with a
different key later can't install over it — people would have to uninstall first. To sign
with a stable key, create one once and keep it safe:

```bash
keytool -genkeypair -v -keystore devti-release.jks -alias devti -keyalg RSA -keysize 4096 -validity 10000
```

Then add four repository secrets (*Settings → Secrets and variables → Actions*):

| Secret | Value |
|---|---|
| `DEVTI_KEYSTORE_B64` | `base64 -w0 devti-release.jks` |
| `DEVTI_KEYSTORE_PASSWORD` | the keystore password |
| `DEVTI_KEY_ALIAS` | `devti` |
| `DEVTI_KEY_PASSWORD` | the key password |

Windows and Linux builds are unsigned (code-signing certificates cost money); the README
explains SmartScreen's "More info → Run anyway".

## 3. Tag

```bash
git tag v1.0.0
```

```bash
git push origin v1.0.0
```

`.github/workflows/release.yml` then builds the Windows installers (`.exe`, `.msi`) and the
`devti` command, the Linux packages (`.deb`, `.rpm`, `.AppImage`), the Android APK and the
example plugin, writes `SHA256SUMS.txt`, and publishes the release with the notes from
`docs/release-notes/`. The iPhone app is built by each person with Xcode (ios/README.md).
