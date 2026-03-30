.PHONY: build run release bundle clean

APP_NAME = VoiceInput
BINARY_NAME = makepad-voice-input
BUNDLE_DIR = target/$(APP_NAME).app
BUNDLE_ID = com.makepad.voice-input

build:
	cargo build -p $(BINARY_NAME)

run:
	cargo run -p $(BINARY_NAME)

release:
	cargo build --release -p $(BINARY_NAME)

bundle: release
	@echo "Creating $(APP_NAME).app bundle..."
	@mkdir -p "$(BUNDLE_DIR)/Contents/MacOS"
	@mkdir -p "$(BUNDLE_DIR)/Contents/Resources"
	@cp "target/release/$(BINARY_NAME)" "$(BUNDLE_DIR)/Contents/MacOS/$(APP_NAME)"
	@echo '<?xml version="1.0" encoding="UTF-8"?>' > "$(BUNDLE_DIR)/Contents/Info.plist"
	@echo '<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">' >> "$(BUNDLE_DIR)/Contents/Info.plist"
	@echo '<plist version="1.0">' >> "$(BUNDLE_DIR)/Contents/Info.plist"
	@echo '<dict>' >> "$(BUNDLE_DIR)/Contents/Info.plist"
	@echo '  <key>CFBundleIdentifier</key><string>$(BUNDLE_ID)</string>' >> "$(BUNDLE_DIR)/Contents/Info.plist"
	@echo '  <key>CFBundleName</key><string>$(APP_NAME)</string>' >> "$(BUNDLE_DIR)/Contents/Info.plist"
	@echo '  <key>CFBundleExecutable</key><string>$(APP_NAME)</string>' >> "$(BUNDLE_DIR)/Contents/Info.plist"
	@echo '  <key>CFBundleVersion</key><string>0.1.0</string>' >> "$(BUNDLE_DIR)/Contents/Info.plist"
	@echo '  <key>CFBundlePackageType</key><string>APPL</string>' >> "$(BUNDLE_DIR)/Contents/Info.plist"
	@echo '  <key>LSUIElement</key><true/>' >> "$(BUNDLE_DIR)/Contents/Info.plist"
	@echo '  <key>NSMicrophoneUsageDescription</key><string>Voice Input needs microphone access for speech recognition.</string>' >> "$(BUNDLE_DIR)/Contents/Info.plist"
	@echo '</dict>' >> "$(BUNDLE_DIR)/Contents/Info.plist"
	@echo '</plist>' >> "$(BUNDLE_DIR)/Contents/Info.plist"
	@echo "Bundle created at $(BUNDLE_DIR)"

clean:
	cargo clean
	@rm -rf "target/$(APP_NAME).app"
