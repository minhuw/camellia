// tests/tests.rs

#[cfg(test)]
mod tests {
    use libxdp_sys::*;

    unsafe extern "C" fn print_fn(
        _level: libbpf_print_level,
        _arg1: *const std::os::raw::c_char,
        _ap: *mut __va_list_tag,
    ) -> std::os::raw::c_int {
        return 0;
    }

    #[test]
    fn test() {
        unsafe {
            // just tests that we can call into the library
            assert!(libxdp_set_print(Some(print_fn as _)).is_some());
        }
    }
}
