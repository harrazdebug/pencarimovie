# Android signing

The Android APK projects are a **local build tree**. `android/` is gitignored and
is not part of the published source — only the built APK is published, as a
GitHub release asset (feed `apk-server-latest`).

## Why

`android/**/app/release-key.jks` is the signing identity every installed copy of
the app trusts. Android will install an APK signed with the same key over the
existing app **without asking the user**, so a published keystore lets anyone
build an APK that silently replaces ours on every device that updates from the
feed. Neither the keystore nor its password may live in this repository.

## Where the pieces live

| Piece                                      | Location                                                         |
| ------------------------------------------ | ---------------------------------------------------------------- |
| `release-key.jks`, `testkey_untrusted.jks` | `android/<app>/app/` on the build machine only                   |
| Keystore passwords                         | the GITIGNORED `android/<app>/local.properties`                  |
| Or, instead of the file                    | env vars `PM_RELEASE_STORE_PASSWORD` / `PM_DEBUG_STORE_PASSWORD` |

`local.properties` is already ignored for every Android project, so the password
never reaches git:

```properties
sdk.dir=C\:\\Users\\you\\AppData\\Local\\Android\\Sdk
pm.release.storePassword=<release keystore password>
pm.debug.storePassword=<testkey password>
```

The build files read them like this, with the environment as an override:

```gradle
def pmSecret = { String key, String envName ->
    def fromEnv = System.getenv(envName)
    return (fromEnv != null && !fromEnv.isEmpty())
        ? fromEnv
        : (pmLocalProps.getProperty(key) ?: '')
}
def pmReleaseStorePassword = pmSecret('pm.release.storePassword', 'PM_RELEASE_STORE_PASSWORD')
```

## Setting up a new build machine

1. Copy `release-key.jks` and `testkey_untrusted.jks` into `android/<app>/app/`
   out of band (password manager / secure transfer). **Never** commit them.
2. Add `pm.release.storePassword` and `pm.debug.storePassword` to that project's
   `local.properties`.
3. `.\gradlew.bat :app:assembleRelease` — this now fails with a keystore error if
   the password is missing rather than shipping an unsigned APK.
4. Publish with `android\release-rust.ps1` (Rust app) or the equivalent script.

CI cannot build a signed APK from this repository by design. That is why the
APK feed is published from a machine that holds the key, rather than from a
GitHub Action.

## Rotation

If the keystore was ever committed, treat it as **compromised** and rotate it.
Rotation is not a silent operation: Android refuses an update signed with a
different key, so a new key means either

- a new `applicationId` (users install a second app and the old one must be
  uninstalled), or
- a Play App Signing / key-upgrade flow, which does not apply to a sideloaded
  APK feed.

Decide the migration path before rotating; do not rotate a key that existing
installs still need to upgrade in place from.
