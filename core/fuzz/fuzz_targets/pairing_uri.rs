//! QR payloads come from the camera, i.e. from anyone. Parsing must never panic and
//! anything accepted must survive a render/parse round trip.
#![no_main]

use devti_core::pairing::PairingUri;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else { return };
    if let Ok(uri) = PairingUri::parse(text) {
        let again = PairingUri::parse(&uri.to_uri()).expect("rendered link parses");
        assert_eq!(again, uri);
    }
});
