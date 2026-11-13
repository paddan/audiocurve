# Build AudioCurve.app (with the embedded AudioCurveAU.appex) without Xcode.
#
# Needs only the Command Line Tools (`xcode-select --install`) and Rust via rustup.
# The Xcode project generated from project.yml keeps working; this Makefile mirrors
# its settings (bundle IDs, versions, deployment target, Swift 6 language mode).
#
#   make release                     build build/release/AudioCurve.app (ad hoc signed)
#   make release ARCHS="arm64 x86_64"  universal build
#   make release SIGN_IDENTITY="Developer ID Application: ..."
#   make install                     copy to /Applications and register the extension
#   make uninstall                   remove it from /Applications again
#   make notarize NOTARY_PROFILE=... notarize and staple (needs a Developer ID identity)
#   make test                        run the Rust and Swift package tests
#   make clean                       remove build/

APP_NAME      := AudioCurve
AU_NAME       := AudioCurveAU
APP_ID        := se.lindefors.audiocurve
AU_ID         := $(APP_ID).$(AU_NAME)
MACOS_MIN     := 27.0
# Single source of truth for the version numbers: project.yml.
MARKETING_VERSION       := $(shell sed -n 's/^ *MARKETING_VERSION: *"\{0,1\}\([^"]*\)"\{0,1\} *$$/\1/p' project.yml)
CURRENT_PROJECT_VERSION := $(shell sed -n 's/^ *CURRENT_PROJECT_VERSION: *"\{0,1\}\([^"]*\)"\{0,1\} *$$/\1/p' project.yml)

ARCHS         ?= arm64
SIGN_IDENTITY ?= -
INSTALL_DIR   ?= /Applications
NOTARY_PROFILE ?=

BUILD_DIR   := build
OBJ_DIR     := $(BUILD_DIR)/obj
OUT_DIR     := $(BUILD_DIR)/release
APP         := $(OUT_DIR)/$(APP_NAME).app
APPEX       := $(APP)/Contents/PlugIns/$(AU_NAME).appex
STAMP       := $(OBJ_DIR)/bundle.stamp

CARGO   ?= $(firstword $(shell command -v cargo 2>/dev/null) $(HOME)/.cargo/bin/cargo)
RUSTUP  ?= $(firstword $(shell command -v rustup 2>/dev/null) $(HOME)/.cargo/bin/rustup)
SDK     := $(shell xcrun --sdk macosx --show-sdk-path)
SWIFTC  := xcrun --sdk macosx swiftc
CLANG   := xcrun --sdk macosx clang
LSREGISTER := /System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister

# The staticlib's objects should target the same minimum macOS as the Swift code.
export MACOSX_DEPLOYMENT_TARGET := $(MACOS_MIN)

rust_triple = $(if $(filter arm64,$(1)),aarch64-apple-darwin,$(1)-apple-darwin)

DSP_SRC   := dsp/Cargo.toml dsp/Cargo.lock $(wildcard dsp/src/*.rs)
CORE_DIR  := Packages/AudioCurveCore/Sources
CORE_SRC  := $(wildcard $(CORE_DIR)/AudioCurveCore/*.swift)
CDSP_DIR  := $(CORE_DIR)/CAudioCurveDSP
CDSP_HDR  := $(CDSP_DIR)/include/audiocurve_dsp.h
AU_SRC    := $(wildcard AU/*.swift AU/UI/*.swift)
APP_SRC   := $(wildcard App/*.swift App/Audio/*.swift)
MODMAP    := $(OBJ_DIR)/modules/module.modulemap

SWIFT_FLAGS = -sdk $(SDK) -target $*-apple-macos$(MACOS_MIN) -swift-version 6 \
              -O -whole-module-optimization -parse-as-library

CODESIGN_FLAGS = --force --sign "$(SIGN_IDENTITY)" $(if $(filter -,$(SIGN_IDENTITY)),,--options runtime --timestamp)

.PHONY: all release install uninstall notarize test clean
# Keep intermediate objects so rebuilds are incremental.
.SECONDARY:
.DEFAULT_GOAL := release

all: release

# Signing runs on every `make release`, so a changed SIGN_IDENTITY always takes effect.
# Sign inside out: the extension (sandboxed, as AUv3 requires) before the app.
release: $(STAMP)
	codesign $(CODESIGN_FLAGS) --entitlements AU/AudioCurveAU.entitlements $(APPEX)
	codesign $(CODESIGN_FLAGS) $(APP)
	codesign --verify --strict --deep $(APP)
	@echo "Built $(APP) ($(ARCHS), signed with '$(SIGN_IDENTITY)')"

# --- Rust DSP --------------------------------------------------------------

$(OBJ_DIR)/%/libaudiocurve_dsp.a: $(DSP_SRC)
	@test -x "$(CARGO)" || { echo "cargo not found; install Rust from https://rustup.rs" >&2; exit 1; }
	@if [ -x "$(RUSTUP)" ] && ! "$(RUSTUP)" target list --installed | grep -qx '$(call rust_triple,$*)'; then \
		"$(RUSTUP)" target add $(call rust_triple,$*); fi
	cd dsp && "$(CARGO)" build --release --locked --target $(call rust_triple,$*)
	@mkdir -p $(@D)
	cp dsp/target/$(call rust_triple,$*)/release/libaudiocurve_dsp.a $@

# --- Swift package AudioCurveCore and its C module ------------------------

# SwiftPM synthesizes this module map for the CAudioCurveDSP target; do the same.
$(MODMAP): $(CDSP_HDR)
	@mkdir -p $(@D)
	printf 'module CAudioCurveDSP {\n    header "%s"\n    export *\n}\n' "$(abspath $(CDSP_HDR))" > $@

$(OBJ_DIR)/%/shim.o: $(CDSP_DIR)/shim.c $(CDSP_HDR)
	@mkdir -p $(@D)
	$(CLANG) -isysroot $(SDK) -target $*-apple-macos$(MACOS_MIN) -O2 -I $(CDSP_DIR)/include -c $< -o $@

# Compiled with -application-extension because it is linked into the extension.
$(OBJ_DIR)/%/AudioCurveCore.o: $(CORE_SRC) $(MODMAP)
	@mkdir -p $(@D)
	$(SWIFTC) $(SWIFT_FLAGS) -application-extension -module-name AudioCurveCore \
		-I $(dir $(MODMAP)) -emit-module -emit-module-path $(@D)/AudioCurveCore.swiftmodule \
		-c -o $@ $(CORE_SRC)

# --- Executables -----------------------------------------------------------

# The SDK's @State macro needs a compiler plugin that only Xcode ships; views use
# the ViewState alias of the property wrapper instead (see App/ViewState.swift).
check_no_state_macro = @if grep -n '@State[[:space:](]' $(1) /dev/null; then \
	echo "error: use @ViewState instead of @State, so the code builds without Xcode (see App/ViewState.swift)" >&2; exit 1; fi

$(OBJ_DIR)/%/$(AU_NAME): $(AU_SRC) $(OBJ_DIR)/%/AudioCurveCore.o $(OBJ_DIR)/%/shim.o $(OBJ_DIR)/%/libaudiocurve_dsp.a
	$(call check_no_state_macro,$(AU_SRC))
	$(SWIFTC) $(SWIFT_FLAGS) -application-extension -module-name $(AU_NAME) \
		-I $(dir $(MODMAP)) -I $(@D) -emit-executable -o $@ $(AU_SRC) \
		$(@D)/AudioCurveCore.o $(@D)/shim.o $(@D)/libaudiocurve_dsp.a \
		-Xlinker -e -Xlinker _NSExtensionMain -Xlinker -dead_strip

$(OBJ_DIR)/%/$(APP_NAME): $(APP_SRC) $(OBJ_DIR)/%/AudioCurveCore.o $(OBJ_DIR)/%/shim.o $(OBJ_DIR)/%/libaudiocurve_dsp.a
	$(call check_no_state_macro,$(APP_SRC))
	@mkdir -p $(@D)
	$(SWIFTC) $(SWIFT_FLAGS) -module-name $(APP_NAME) \
		-I $(dir $(MODMAP)) -I $(@D) -emit-executable -o $@ $(APP_SRC) \
		$(@D)/AudioCurveCore.o $(@D)/shim.o $(@D)/libaudiocurve_dsp.a \
		-Xlinker -dead_strip

# --- Bundles ---------------------------------------------------------------

# Replace the Xcode build-setting references in an Info.plist.
# $(1) source plist, $(2) destination, $(3) product name, $(4) bundle ID
define make_plist
	@mkdir -p $(dir $(2))
	sed -e 's/$$(EXECUTABLE_NAME)/$(3)/g' \
	    -e 's/$$(PRODUCT_NAME)/$(3)/g' \
	    -e 's/$$(PRODUCT_MODULE_NAME)/$(3)/g' \
	    -e 's/$$(PRODUCT_BUNDLE_IDENTIFIER)/$(4)/g' \
	    -e 's/$$(MARKETING_VERSION)/$(MARKETING_VERSION)/g' \
	    -e 's/$$(CURRENT_PROJECT_VERSION)/$(CURRENT_PROJECT_VERSION)/g' \
	    -e 's/$$(MACOSX_DEPLOYMENT_TARGET)/$(MACOS_MIN)/g' $(1) > $(2)
	plutil -replace LSMinimumSystemVersion -string $(MACOS_MIN) $(2)
	plutil -replace CFBundleSupportedPlatforms -json '["MacOSX"]' $(2)
	@if grep -q '\$$(' $(2); then echo "$(2): unresolved build setting:" >&2; grep '\$$(' $(2) >&2; exit 1; fi
	plutil -lint -s $(2)
endef


$(STAMP): $(foreach a,$(ARCHS),$(OBJ_DIR)/$(a)/$(AU_NAME) $(OBJ_DIR)/$(a)/$(APP_NAME)) \
          App/Info.plist AU/Info.plist App/Localizable.xcstrings AU/Localizable.xcstrings \
          scripts/xcstrings2strings.py project.yml Makefile assets/AppIcon.icns
	rm -rf $(APP)
	mkdir -p $(APP)/Contents/MacOS $(APP)/Contents/Resources $(APPEX)/Contents/MacOS $(APPEX)/Contents/Resources
	lipo -create -output $(APP)/Contents/MacOS/$(APP_NAME) $(foreach a,$(ARCHS),$(OBJ_DIR)/$(a)/$(APP_NAME))
	lipo -create -output $(APPEX)/Contents/MacOS/$(AU_NAME) $(foreach a,$(ARCHS),$(OBJ_DIR)/$(a)/$(AU_NAME))
	$(call make_plist,App/Info.plist,$(APP)/Contents/Info.plist,$(APP_NAME),$(APP_ID))
	$(call make_plist,AU/Info.plist,$(APPEX)/Contents/Info.plist,$(AU_NAME),$(AU_ID))
	printf 'APPL????' > $(APP)/Contents/PkgInfo
	# App icon: the Icon Composer icon needs Xcode's actool, so use the prebuilt dark .icns.
	cp assets/AppIcon.icns $(APP)/Contents/Resources/AppIcon.icns
	plutil -replace CFBundleIconFile -string AppIcon $(APP)/Contents/Info.plist
	python3 scripts/xcstrings2strings.py App/Localizable.xcstrings $(APP)/Contents/Resources
	python3 scripts/xcstrings2strings.py AU/Localizable.xcstrings $(APPEX)/Contents/Resources
	@touch $@


# --- Install, notarize, test, clean ----------------------------------------

install: release
	@if [ -d "$(INSTALL_DIR)/$(APP_NAME).app" ]; then \
		echo "Replacing $(INSTALL_DIR)/$(APP_NAME).app"; rm -rf "$(INSTALL_DIR)/$(APP_NAME).app"; fi
	ditto $(APP) "$(INSTALL_DIR)/$(APP_NAME).app"
	$(LSREGISTER) -f -R "$(INSTALL_DIR)/$(APP_NAME).app"
	pluginkit -a "$(INSTALL_DIR)/$(APP_NAME).app/Contents/PlugIns/$(AU_NAME).appex"
	@sleep 1; pluginkit -m -v -i $(AU_ID) || true
	@echo "Installed. Open $(APP_NAME) once, or restart your audio host, to pick up the plugin."

uninstall:
	-pluginkit -r "$(INSTALL_DIR)/$(APP_NAME).app/Contents/PlugIns/$(AU_NAME).appex"
	-$(LSREGISTER) -u "$(INSTALL_DIR)/$(APP_NAME).app"
	rm -rf "$(INSTALL_DIR)/$(APP_NAME).app"

notarize: release
	@if [ "$(SIGN_IDENTITY)" = "-" ]; then echo "notarize needs SIGN_IDENTITY=\"Developer ID Application: ...\"" >&2; exit 1; fi
	@if [ -z "$(NOTARY_PROFILE)" ]; then echo "notarize needs NOTARY_PROFILE=<name> (see: xcrun notarytool store-credentials)" >&2; exit 1; fi
	ditto -c -k --keepParent $(APP) $(BUILD_DIR)/$(APP_NAME).zip
	xcrun notarytool submit $(BUILD_DIR)/$(APP_NAME).zip --keychain-profile "$(NOTARY_PROFILE)" --wait
	xcrun stapler staple $(APP)
	rm -f $(BUILD_DIR)/$(APP_NAME).zip

test:
	cd dsp && "$(CARGO)" test
	cd Packages/AudioCurveCore && swift test

clean:
	rm -rf $(BUILD_DIR)
