#!/usr/bin/env bash
#
# testflight-release.sh — build, sign and upload the Overseer phone app to TestFlight (iOS).
#
# Runs locally on a Mac with Xcode. It builds phone/ from the CURRENT checkout (build from the
# real source, never a stale copy), archives it, exports an App Store .ipa signed with the
# STATION 42 distribution cert + the "Overseer App Store" provisioning profile, validates it, and
# uploads it to TestFlight. Auto-distribution to the internal group then delivers it to testers.
#
# ─── THE BUILD-NUMBER RULE ────────────────────────────────────────────────────────────────────
# App Store Connect REJECTS an upload whose build number (CFBundleVersion) reuses one already
# uploaded for the same marketing version ("The bundle version must be higher than the previously
# uploaded version"). This script stamps a timestamp build number (YYYYMMDDHHMM) by default, so
# every upload is unique and strictly increasing — you never hit that error. Override with
# BUILD_NUMBER=... if you want a specific one (it must still be higher than the last upload).
#
# ─── ONE-TIME SETUP (owner) ───────────────────────────────────────────────────────────────────
#   • Xcode installed; the STATION 42 "Apple Distribution" cert in the login keychain.
#   • The "Overseer App Store" provisioning profile installed in
#     ~/Library/MobileDevice/Provisioning Profiles/ (create once in the Developer portal:
#     Profiles → + → App Store Connect → App ID com.beelol.overseer.phone → the distribution cert).
#   • An App Store Connect API key (App Manager role) saved at
#     ~/.appstoreconnect/private_keys/AuthKey_<ASC_KEY_ID>.p8  (downloaded once from
#     Users and Access → Integrations → App Store Connect API). Never commit the .p8.
#
# ─── USAGE ────────────────────────────────────────────────────────────────────────────────────
#   scripts/testflight-release.sh                # build + upload with a timestamp build number
#   scripts/testflight-release.sh --no-upload    # build + validate only, skip the upload
#   BUILD_NUMBER=42 scripts/testflight-release.sh # force a specific build number
#
# Env (all have defaults for this project):
#   ASC_KEY_ID    App Store Connect API key id        (default: VAT46LATWS)
#   ASC_ISSUER_ID App Store Connect API issuer id     (default: c419168b-6bd1-4b72-bbed-78617b507726)
#   TEAM_ID       Apple Developer team id             (default: FQ6YGD7554)
#   PROFILE_NAME  App Store provisioning profile name (default: "Overseer App Store")
#   BUILD_NUMBER  CFBundleVersion to stamp            (default: date +%Y%m%d%H%M)
#   P8_PATH       path to the API key .p8             (default: ~/.appstoreconnect/private_keys/AuthKey_<ASC_KEY_ID>.p8)
#
set -euo pipefail
export LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8

ASC_KEY_ID="${ASC_KEY_ID:-VAT46LATWS}"
ASC_ISSUER_ID="${ASC_ISSUER_ID:-c419168b-6bd1-4b72-bbed-78617b507726}"
TEAM_ID="${TEAM_ID:-FQ6YGD7554}"
PROFILE_NAME="${PROFILE_NAME:-Overseer App Store}"
BUILD_NUMBER="${BUILD_NUMBER:-$(date +%Y%m%d%H%M)}"
UPLOAD=1
[ "${1:-}" = "--no-upload" ] && UPLOAD=0

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
phone="$repo_root/phone"
[ -d "$phone" ] || { echo "error: $phone not found (run from a checkout that has the phone app)"; exit 1; }
P8_PATH="${P8_PATH:-$HOME/.appstoreconnect/private_keys/AuthKey_$ASC_KEY_ID.p8}"
[ -f "$P8_PATH" ] || { echo "error: API key not found at $P8_PATH — download it from App Store Connect first"; exit 1; }

work="$(mktemp -d /tmp/overseer-testflight.XXXXXX)"
archive="$work/Overseer.xcarchive"
export_dir="$work/export"
echo "▸ phone:        $phone"
echo "▸ build number: $BUILD_NUMBER"
echo "▸ work dir:     $work"

cd "$phone"
echo "▸ installing deps…";        npm install --no-audit --no-fund >/dev/null
echo "▸ generating brand assets…"; node scripts/gen-assets.mjs >/dev/null
echo "▸ expo prebuild (ios)…";     OVERSEER_BUILD_NUMBER="$BUILD_NUMBER" npx expo prebuild -p ios --clean >/dev/null
# phone/app.config.ts takes the build number from OVERSEER_BUILD_NUMBER; prebuild writes it into
# Info.plist, where CURRENT_PROJECT_VERSION alone would not reach.
stamped=$(/usr/libexec/PlistBuddy -c "Print :CFBundleVersion" ios/Overseer/Info.plist)
[ "$stamped" = "$BUILD_NUMBER" ] || { echo "✗ Info.plist has build $stamped, not $BUILD_NUMBER" >&2; exit 1; }

# API key must be findable by altool too.
mkdir -p "$HOME/.appstoreconnect/private_keys"
cp -f "$P8_PATH" "$HOME/.appstoreconnect/private_keys/AuthKey_$ASC_KEY_ID.p8"

echo "▸ archiving (signed)…"
xcodebuild -workspace ios/Overseer.xcworkspace -scheme Overseer -configuration Release \
  -sdk iphoneos -destination 'generic/platform=iOS' -archivePath "$archive" \
  -allowProvisioningUpdates \
  -authenticationKeyPath "$P8_PATH" -authenticationKeyID "$ASC_KEY_ID" -authenticationKeyIssuerID "$ASC_ISSUER_ID" \
  DEVELOPMENT_TEAM="$TEAM_ID" CODE_SIGN_STYLE=Automatic \
  CURRENT_PROJECT_VERSION="$BUILD_NUMBER" \
  archive

cat > "$work/ExportOptions.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>method</key><string>app-store-connect</string>
  <key>teamID</key><string>$TEAM_ID</string>
  <key>signingStyle</key><string>manual</string>
  <key>signingCertificate</key><string>Apple Distribution</string>
  <key>provisioningProfiles</key><dict>
    <key>com.beelol.overseer.phone</key><string>$PROFILE_NAME</string>
  </dict>
  <key>uploadSymbols</key><true/>
  <key>manageAppVersionAndBuildNumber</key><false/>
</dict></plist>
PLIST

echo "▸ exporting .ipa (manual distribution signing)…"
xcodebuild -exportArchive -archivePath "$archive" -exportPath "$export_dir" \
  -exportOptionsPlist "$work/ExportOptions.plist"

ipa="$(ls "$export_dir"/*.ipa | head -1)"
echo "▸ validating…"
xcrun altool --validate-app -f "$ipa" -t ios --apiKey "$ASC_KEY_ID" --apiIssuer "$ASC_ISSUER_ID"

if [ "$UPLOAD" = "1" ]; then
  echo "▸ uploading to TestFlight…"
  xcrun altool --upload-app -f "$ipa" -t ios --apiKey "$ASC_KEY_ID" --apiIssuer "$ASC_ISSUER_ID"
  echo "✓ uploaded build $BUILD_NUMBER — it will process, then auto-distribute to the internal group."
else
  echo "✓ built + validated (upload skipped): $ipa"
fi
