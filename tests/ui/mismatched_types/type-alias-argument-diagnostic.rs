extern "C" fn char_type(ch: core::ffi::c_char) -> core::ffi::c_char {
    ch
}

fn main() {
    let arg: u16 = b"char"[0].into();
    char_type(arg); //~ ERROR mismatched types
}
