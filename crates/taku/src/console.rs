#[cfg(windows)]
pub fn init() {
    windows::enable();
}

#[cfg(not(windows))]
pub fn init() {}

#[cfg(windows)]
mod windows {
    use std::ffi::c_void;

    type Dword = u32;
    type Handle = *mut c_void;
    type Bool = i32;

    const CP_UTF8: u32 = 65001;
    const STD_OUTPUT_HANDLE: Dword = -11i32 as Dword;
    const STD_ERROR_HANDLE: Dword = -12i32 as Dword;
    const INVALID_HANDLE_VALUE: Handle = -1isize as Handle;
    const ENABLE_VIRTUAL_TERMINAL_PROCESSING: Dword = 0x0004;

    unsafe extern "system" {
        fn SetConsoleOutputCP(code_page: u32) -> Bool;
        fn SetConsoleCP(code_page: u32) -> Bool;
        fn GetStdHandle(std_handle: Dword) -> Handle;
        fn GetConsoleMode(console: Handle, mode: *mut Dword) -> Bool;
        fn SetConsoleMode(console: Handle, mode: Dword) -> Bool;
    }

    pub fn enable() {
        unsafe {
            SetConsoleOutputCP(CP_UTF8);
            SetConsoleCP(CP_UTF8);
            enable_vt(STD_OUTPUT_HANDLE);
            enable_vt(STD_ERROR_HANDLE);
        }
    }

    unsafe fn enable_vt(std_handle: Dword) {
        unsafe {
            let handle = GetStdHandle(std_handle);
            if handle.is_null() || handle == INVALID_HANDLE_VALUE {
                return;
            }
            let mut mode: Dword = 0;
            if GetConsoleMode(handle, &mut mode) == 0 {
                return;
            }
            SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING);
        }
    }
}
