#!/usr/bin/env bash
# Builds Devti Core for iOS as DevtiCore.xcframework and generates its Swift bindings.
#
#   ./build-core.sh            device + simulator, release (what Xcode needs to run on a phone)
#   ./build-core.sh sim debug  simulator only, debug (what CI uses to build and test quickly)
set -euo pipefail
cd "$(dirname "$0")/.."
WHICH="${1:-all}"
PROFILE="${2:-release}"
FLAG=$([ "$PROFILE" = release ] && echo --release || true)

case "$WHICH" in
  sim) TARGETS=(aarch64-apple-ios-sim) ;;
  *) TARGETS=(aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios) ;;
esac
for t in "${TARGETS[@]}"; do
  rustup target add "$t" >/dev/null
  # iOS 16 is the oldest system the app supports.
  IPHONEOS_DEPLOYMENT_TARGET=16.0 cargo build -p devti-ffi --lib $FLAG --target "$t"
done

# Swift bindings come from the host build of the same crate.
cargo build -p devti-ffi --lib
HOST_LIB=$(ls target/debug/libdevti_ffi.dylib target/debug/libdevti_ffi.so 2>/dev/null | head -1)
GEN=ios/Generated
rm -rf "$GEN" ios/Frameworks/DevtiCore.xcframework
mkdir -p "$GEN" ios/build/headers ios/Frameworks
cargo run -q -p devti-ffi --features bindgen --bin uniffi-bindgen -- \
  generate --library "$HOST_LIB" --language swift --out-dir ios/build/uniffi
mv ios/build/uniffi/devti_ffi.swift "$GEN/"
cp ios/build/uniffi/devti_ffiFFI.h ios/build/headers/
cp ios/build/uniffi/devti_ffiFFI.modulemap ios/build/headers/module.modulemap

ARGS=()
if [ "$WHICH" = sim ]; then
  ARGS+=(-library "target/aarch64-apple-ios-sim/$PROFILE/libdevti_ffi.a" -headers ios/build/headers)
else
  mkdir -p "target/ios-sim-universal/$PROFILE"
  lipo -create "target/aarch64-apple-ios-sim/$PROFILE/libdevti_ffi.a" "target/x86_64-apple-ios/$PROFILE/libdevti_ffi.a" \
    -output "target/ios-sim-universal/$PROFILE/libdevti_ffi.a"
  ARGS+=(-library "target/aarch64-apple-ios/$PROFILE/libdevti_ffi.a" -headers ios/build/headers)
  ARGS+=(-library "target/ios-sim-universal/$PROFILE/libdevti_ffi.a" -headers ios/build/headers)
fi
xcodebuild -create-xcframework "${ARGS[@]}" -output ios/Frameworks/DevtiCore.xcframework
echo "Built ios/Frameworks/DevtiCore.xcframework and ios/Generated/devti_ffi.swift"
