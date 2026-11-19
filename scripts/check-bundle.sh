#!/bin/sh
# Checks a built AudioCurve.app: the executable is there and runs on Apple silicon, the
# Info.plist has the keys the app needs, the AUv3 extension is embedded, and the signature
# verifies.
#
# Usage: scripts/check-bundle.sh build/release/AudioCurve.app
set -eu

if [ $# -ne 1 ]; then
	echo "usage: $0 <AudioCurve.app>" >&2
	exit 2
fi
APP=$1
PLIST="$APP/Contents/Info.plist"
EXE="$APP/Contents/MacOS/AudioCurve"
APPEX="$APP/Contents/PlugIns/AudioCurveAU.appex"
failed=0

fail() {
	echo "FAIL: $*" >&2
	failed=1
}

plist_value() {
	/usr/libexec/PlistBuddy -c "Print :$1" "$PLIST" 2>/dev/null
}

if [ ! -f "$EXE" ]; then
	fail "$EXE is missing"
elif ! lipo -archs "$EXE" | tr ' ' '\n' | grep -qx arm64; then
	fail "$EXE has no arm64 slice (it has: $(lipo -archs "$EXE"))"
fi

if [ ! -f "$PLIST" ]; then
	fail "$PLIST is missing"
else
	[ "$(plist_value CFBundleIdentifier)" = "se.lindefors.audiocurve" ] ||
		fail "CFBundleIdentifier is '$(plist_value CFBundleIdentifier)', not se.lindefors.audiocurve"
	[ "$(plist_value CFBundleExecutable)" = "AudioCurve" ] ||
		fail "CFBundleExecutable is '$(plist_value CFBundleExecutable)', not AudioCurve"
	[ "$(plist_value LSUIElement)" = "true" ] || fail "LSUIElement is not true"
	[ -n "$(plist_value NSAudioCaptureUsageDescription)" ] || fail "NSAudioCaptureUsageDescription is missing"
fi

[ -d "$APPEX" ] || fail "$APPEX is missing"

codesign --verify --deep --strict "$APP" || fail "codesign --verify --deep --strict failed"

if [ $failed -ne 0 ]; then
	exit 1
fi
echo "OK: $APP"
