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

# NDK location: explicit env wins, else the newest NDK under a standard SDK root.
find_ndk() {
  local sdk
  for sdk in "${ANDROID_SDK_ROOT:-}" "${ANDROID_HOME:-}" "$HOME/Library/Android/sdk" "$HOME/Android/Sdk"; do
    [ -n "$sdk" ] && [ -d "$sdk/ndk" ] || continue
    ls -1d "$sdk"/ndk/* 2>/dev/null | sort -V | tail -1 && return 0
  done
  return 1
}
NDK="${ANDROID_NDK_HOME:-${ANDROID_NDK_ROOT:-$(find_ndk || true)}}"
if [ -z "$NDK" ] || [ ! -d "$NDK" ]; then
  echo "error: Android NDK not found. Set ANDROID_NDK_HOME to your NDK directory." >&2
  exit 1
fi

# The NDK ships x86_64 host binaries on both platforms (macOS runs them via Rosetta on Apple
# Silicon), so the host tag depends only on the OS.
case "$(uname -s)" in
  Darwin) HOST_TAG="darwin-x86_64"; HOST_LIB_EXT="dylib" ;;
  Linux)  HOST_TAG="linux-x86_64";  HOST_LIB_EXT="so" ;;
  *) echo "error: unsupported build host $(uname -s); use macOS or Linux." >&2; exit 1 ;;
esac
TOOLCHAIN="$NDK/toolchains/llvm/prebuilt/$HOST_TAG/bin"
if [ ! -d "$TOOLCHAIN" ]; then
  echo "error: no toolchain at $TOOLCHAIN (is $NDK a valid NDK?)" >&2
  exit 1
fi
API=26

# Which ABIs to build. CI overrides this to a single ABI to keep PR builds fast:
#   ABIS=x86_64 ./scripts/build-android-lib.sh
ABIS="${ABIS:-arm64-v8a armeabi-v7a x86_64}"

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

for abi in $ABIS; do
  case "$abi" in
    arm64-v8a)   build_abi aarch64-linux-android   arm64-v8a   "aarch64-linux-android${API}-clang"    AARCH64_LINUX_ANDROID ;;
    armeabi-v7a) build_abi armv7-linux-androideabi armeabi-v7a "armv7a-linux-androideabi${API}-clang" ARMV7_LINUX_ANDROIDEABI ;;
    x86_64)      build_abi x86_64-linux-android    x86_64      "x86_64-linux-android${API}-clang"     X86_64_LINUX_ANDROID ;;
    *) echo "error: unknown ABI '$abi' (want arm64-v8a, armeabi-v7a, or x86_64)" >&2; exit 1 ;;
  esac
done

echo "==> generating Kotlin bindings"
BINDINGS="android/app/src/main/java"
mkdir -p "$BINDINGS"
cargo build -p meshcore-ffi >/dev/null 2>&1
cargo run -q -p meshcore-ffi --bin uniffi-bindgen -- \
  generate --library "target/debug/libmeshcore_ffi.$HOST_LIB_EXT" --language kotlin --out-dir "$BINDINGS"
echo "==> done: jniLibs for $ABIS + Kotlin bindings"
