//! Bytes from a Bluetooth accessory are untrusted: no framing or profile reader may panic.
#![no_main]

use devti_core::accessory::codec::Framer;
use devti_core::accessory::{Framing, builtin_profiles};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    for framing in [Framing::Raw, Framing::Sony, Framing::Galaxy, Framing::Huawei, Framing::Nothing] {
        let mut f = Framer::new(framing);
        for chunk in data.chunks(7) {
            for payload in f.decode(chunk) {
                for p in builtin_profiles() {
                    let _ = p.read(&payload);
                }
            }
        }
    }
});
