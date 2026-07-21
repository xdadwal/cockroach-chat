#!/usr/bin/env bash
# Cross-compiles meshcore-ffi to .so libraries for all common Android ABIs and drops them into
# the app's jniLibs, plus regenerates the Kotlin UniFFI bindings. Run before ./gradlew.
#
# Produces a universal APK that installs on virtually any Android device:
#   arm64-v8a    — modern phones (default)
#   armeabi-v7a  — older / cheap 32-bit devices
#   x86_64       — emulators
#
# In a production setup this is wired into Gradle via org.mozilla.rust-android-gradle; a plain
# script keeps the toolchain legible and CI-friendly for now.
set -euo pipefail
cd "$(dirname "$0")/.."

NDK="${ANDROID_NDK_HOME:-$HOME/Library/Android/sdk/ndk/28.2.13676358}"
HOST_TAG="darwin-x86_64" # NDK ships x86_64 host binaries (run via Rosetta on Apple Silicon)
TOOLCHAIN="$NDK/toolchains/llvm/prebuilt/$HOST_TAG/bin"
API=26

# SQLCipher's vendored OpenSSL (via meshcore-store) is cross-compiled from source, so openssl-src
# and the cc crate need the NDK on PATH and the ANDROID_NDK_ROOT env set.
export ANDROID_NDK_ROOT="$NDK"
export ANDROID_NDK_HOME="$NDK"
export PATH="$TOOLCHAIN:$PATH"

# triple : jniLibs abi dir : clang wrapper prefix : cargo linker env var stem
build_abi() {
  local triple="$1" abi="$2" clang="$3" var="$4"
  local tu="${triple//-/_}"
  echo "==> building $triple -> $abi"
  export "CARGO_TARGET_${var}_LINKER=$TOOLCHAIN/$clang"
  export "CC_${tu}=$TOOLCHAIN/$clang"       # for the cc crate + openssl-src
  export "AR_${tu}=$TOOLCHAIN/llvm-ar"
  cargo build -p meshcore-ffi --target "$triple" --release
  mkdir -p "android/app/src/main/jniLibs/$abi"
  cp "target/$triple/release/libmeshcore_ffi.so" "android/app/src/main/jniLibs/$abi/"
}

build_abi aarch64-linux-android   arm64-v8a   "aarch64-linux-android${API}-clang"    AARCH64_LINUX_ANDROID
build_abi armv7-linux-androideabi armeabi-v7a "armv7a-linux-androideabi${API}-clang" ARMV7_LINUX_ANDROIDEABI
build_abi x86_64-linux-android    x86_64      "x86_64-linux-android${API}-clang"     X86_64_LINUX_ANDROID

echo "==> generating Kotlin bindings"
BINDINGS="android/app/src/main/java"
mkdir -p "$BINDINGS"
cargo build -p meshcore-ffi >/dev/null 2>&1
cargo run -q -p meshcore-ffi --bin uniffi-bindgen -- \
  generate --library target/debug/libmeshcore_ffi.dylib --language kotlin --out-dir "$BINDINGS"
echo "==> done: jniLibs for arm64-v8a, armeabi-v7a, x86_64 + Kotlin bindings"
