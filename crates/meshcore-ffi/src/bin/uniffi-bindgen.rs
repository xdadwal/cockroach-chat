//! Thin wrapper so `cargo run --bin uniffi-bindgen -- ...` drives UniFFI's binding generator
//! (used by the Android build to emit Kotlin from the compiled library).
fn main() {
    uniffi::uniffi_bindgen_main()
}
