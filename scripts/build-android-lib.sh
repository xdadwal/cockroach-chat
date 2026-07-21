#!/usr/bin/env bash
# Cross-compiles meshcore-ffi to a .so for the Android emulator/device (arm64-v8a) and drops it
# into the app's jniLibs, plus regenerates the Kotlin UniFFI bindings. Run before ./gradlew.
#
# In a production setup this is wired into Gradle via org.mozilla.rust-android-gradle; a plain
# script keeps the toolchain legible and CI-friendly for now.
set -euo pipefail
cd "$(dirname "$0")/.."

NDK="${ANDROID_NDK_HOME:-$HOME/Library/Android/sdk/ndk/28.2.13676358}"
HOST_TAG="darwin-x86_64" # NDK ships x86_64 host binaries (run via Rosetta on Apple Silicon)
TOOLCHAIN="$NDK/toolchains/llvm/prebuilt/$HOST_TAG/bin"
API=26
ABI_DIR="android/app/src/main/jniLibs/arm64-v8a"

export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$TOOLCHAIN/aarch64-linux-android${API}-clang"
export CC_aarch64_linux_android="$TOOLCHAIN/aarch64-linux-android${API}-clang"
export AR_aarch64_linux_android="$TOOLCHAIN/llvm-ar"

echo "==> building meshcore-ffi for aarch64-linux-android"
cargo build -p meshcore-ffi --target aarch64-linux-android --release

mkdir -p "$ABI_DIR"
cp target/aarch64-linux-android/release/libmeshcore_ffi.so "$ABI_DIR/"
echo "==> copied libmeshcore_ffi.so -> $ABI_DIR"

echo "==> generating Kotlin bindings"
BINDINGS="android/app/src/main/java"
mkdir -p "$BINDINGS"
# Generate from the host build (metadata is identical across targets).
cargo build -p meshcore-ffi >/dev/null 2>&1
cargo run -q -p meshcore-ffi --bin uniffi-bindgen -- \
  generate --library target/debug/libmeshcore_ffi.dylib --language kotlin --out-dir "$BINDINGS"
echo "==> Kotlin bindings written under $BINDINGS/uniffi/"
