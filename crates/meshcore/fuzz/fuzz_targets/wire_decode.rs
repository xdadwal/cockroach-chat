#![no_main]
//! Fuzzes the packet decoder — the parser that sees fully untrusted bytes off the air.
//!
//! A signature-parsing buffer overflow in the equivalent code is what broke bitchat, so this
//! target exists specifically to keep `Packet::decode` total: any input must produce `Ok` or a
//! `WireError`, never a panic, and never an out-of-bounds read.

use libfuzzer_sys::fuzz_target;
use meshcore::wire::Packet;

fuzz_target!(|data: &[u8]| {
    if let Ok(packet) = Packet::decode(data) {
        // Round-trip: anything we accepted must re-encode and decode back to the same packet.
        // A mismatch means the decoder accepted something the encoder can't represent — the
        // shape of bug that lets two peers disagree about what a packet says.
        let reencoded = packet.encode();
        match Packet::decode(&reencoded) {
            Ok(again) => assert_eq!(packet, again, "re-encode/decode changed the packet"),
            Err(e) => panic!("re-encoding a decoded packet produced undecodable bytes: {e:?}"),
        }
    }
});
