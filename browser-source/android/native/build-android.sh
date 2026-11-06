#!/usr/bin/env bash
# Build only on a Linux development/hosted runner with the pinned NDK installed.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
: "${ANDROID_NDK_HOME:?Set ANDROID_NDK_HOME to the installed Android NDK}"
toolchain="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin"
test -x "$toolchain/llvm-ar" || { echo 'Linux Android NDK toolchain not found' >&2; exit 1; }
for entry in \
  'aarch64-linux-android:arm64-v8a:aarch64-linux-android' \
  'armv7-linux-androideabi:armeabi-v7a:armv7a-linux-androideabi' \
  'x86_64-linux-android:x86_64:x86_64-linux-android'; do
  IFS=: read -r target abi clang <<< "$entry"
  linker="$toolchain/${clang}26-clang"
  test -x "$linker" || { echo "Missing linker: $linker" >&2; exit 1; }
  target_key="${target^^}"
  target_key="${target_key//-/_}"
  env "CARGO_TARGET_${target_key}_LINKER=$linker" \
    "CC_${target//-/_}=$linker" \
    "AR_${target//-/_}=$toolchain/llvm-ar" \
    RUSTFLAGS='-C link-arg=-Wl,-z,max-page-size=16384 -C link-arg=-Wl,-z,common-page-size=16384' \
    cargo build --manifest-path "$root/native/Cargo.toml" --release --locked --target "$target"
  "$toolchain/llvm-readelf" --wide --program-headers "$root/native/target/$target/release/libpip_adblock.so" \
    | python3 -c 'import sys; a=[int(l.split()[-1],16) for l in sys.stdin if l.lstrip().startswith("LOAD ")]; assert a and all(n >= 16384 and n & (n-1) == 0 for n in a), a'
  mkdir -p "$root/app/src/main/jniLibs/$abi"
  cp "$root/native/target/$target/release/libpip_adblock.so" "$root/app/src/main/jniLibs/$abi/libpip_adblock.so"
done
