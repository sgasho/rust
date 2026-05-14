extern "C" fn char_pair(left: core::ffi::c_char, right: core::ffi::c_char) {
    let _ = (left, right);
}

fn main() {
    let left: u16 = b"left"[0].into();
    let right: u16 = b"right"[0].into();
    char_pair(left, right); //~ ERROR arguments to this function are incorrect
}
