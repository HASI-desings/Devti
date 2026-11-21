//! Every message type parsed from untrusted bytes must never panic, and anything that
//! parses must re-encode and parse back to the same value.
#![no_main]

use devti_core::protocol::{ControlMsg, PairMsg, StreamOpen, decode, encode};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(m) = decode::<ControlMsg>(data) {
        let f = encode(&m).expect("re-encode");
        assert_eq!(decode::<ControlMsg>(&f[4..]).expect("re-decode"), m);
    }
    if let Ok(m) = decode::<PairMsg>(data) {
        let f = encode(&m).expect("re-encode");
        assert_eq!(decode::<PairMsg>(&f[4..]).expect("re-decode"), m);
    }
    let _ = decode::<StreamOpen>(data);
});
