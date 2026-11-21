//! Feeds arbitrary bytes through the async frame reader with a small limit, as a
//! hostile peer would on a QUIC stream.
#![no_main]

use devti_core::protocol::{ControlMsg, read_message};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let rt = tokio_rt();
    rt.block_on(async {
        let mut reader = data;
        for _ in 0..16 {
            match read_message::<_, ControlMsg>(&mut reader, 4096).await {
                Ok(Some(_)) => continue,
                _ => break,
            }
        }
    });
});

fn tokio_rt() -> &'static tokio::runtime::Runtime {
    use std::sync::OnceLock;
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| tokio::runtime::Builder::new_current_thread().build().unwrap())
}
