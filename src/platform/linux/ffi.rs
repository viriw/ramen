use std::{ffi, mem::transmute, os::raw, ptr};

pub(super) struct Error(raw::c_int);

const XCB_WINDOW_CLASS_INPUT_OUTPUT: u16 = 1;

pub(super) type XcbColourMap = u32;
pub(super) type XcbVisualId = u32;
pub(super) type XcbWindow = u32;

#[repr(C)]
pub(super) struct XcbGenericError {
    response_type: u8,
    error_code: u8,
    sequence: u16,
    resource_id: u32,
    minor_code: u16,
    major_code: u8,
    _pad0: u8,
    _pad: [u32; 5],
    full_sequence: u32,
}

#[repr(C)]
pub(super) struct Cookie {
    seq: raw::c_uint,
}

/// Helps you create C-compatible string literals, like `c_string!("Hello!")` -> `b"Hello!\0"`.
macro_rules! c_string {
    ($s:expr) => {
        concat!($s, "\0").as_ptr().cast()
    };
}

/// Calls dlerror, returning the error string or None if there's no error
pub(super) fn dl_error() -> Option<&'static str> {
    unsafe {
        let start = libc::dlerror() as *mut u8;
        if start.is_null() {
            None
        } else {
        let mut count = 0;
        while *start.add(count) != 0 {
            count += 1;
        }
        Some(std::str::from_utf8_unchecked(std::slice::from_raw_parts(start, count)))
        }
    }
}

// Dummy function which will be pointed to by an invalid Xcb struct
unsafe extern "C" fn do_not_call() -> ! {
    panic!("XCB function was called on invalid Xcb struct");
}

/// Referent type for xcb_connection_t
enum ConnectionPtr {}

/// XCB connection wrapper
pub(super) struct Xcb {
    connection: *mut ConnectionPtr,
    screen: *mut Screen,
    request_check: unsafe extern "C" fn(*mut ConnectionPtr, Cookie) -> *mut XcbGenericError,
    connection_has_error: unsafe extern "C" fn(*mut ConnectionPtr) -> raw::c_int,
    disconnect: unsafe extern "C" fn(*mut ConnectionPtr),
    flush: unsafe extern "C" fn(*mut ConnectionPtr) -> raw::c_int,
    generate_id: unsafe extern "C" fn(*mut ConnectionPtr) -> u32,
    create_window: unsafe extern "C" fn(*mut ConnectionPtr, u8, XcbWindow, XcbWindow, i16, i16, u16, u16, u16, u16, XcbVisualId, u32, *const ffi::c_void) -> Cookie,
    map_window: unsafe extern "C" fn(*mut ConnectionPtr, XcbWindow) -> Cookie,
}
unsafe impl Send for Xcb {}
unsafe impl Sync for Xcb {}
impl Drop for Xcb {
    fn drop(&mut self) {
        // Note: "If `c` is `NULL`, nothing is done" - an XCB header
        unsafe { (self.disconnect)(self.connection) };
    }
}
impl Xcb {
    /// If there's a problem during setup, this function will be called to create an Xcb in an invalid state.
    fn invalid() -> Self {
        Self {
            connection: ptr::null_mut(),
            screen: ptr::null_mut(),
            request_check: unsafe { transmute(do_not_call as unsafe extern "C" fn() -> !) },
            connection_has_error: unsafe { transmute(do_not_call as unsafe extern "C" fn() -> !) },
            disconnect: unsafe { transmute(do_not_call as unsafe extern "C" fn() -> !) },
            flush: unsafe { transmute(do_not_call as unsafe extern "C" fn() -> !) },
            generate_id: unsafe { transmute(do_not_call as unsafe extern "C" fn() -> !) },
            create_window: unsafe { transmute(do_not_call as unsafe extern "C" fn() -> !) },
            map_window: unsafe { transmute(do_not_call as unsafe extern "C" fn() -> !) },
        }
    }

    /// Checks if the connection is valid. An invalid connection usually means setup has not been successful,
    /// but may also mean the connection has shut down due to a fatal error. Further function calls to a
    /// connection in this state will have no effect.
    /// 
    /// See manual page on `xcb_connection_has_error` for more information.
    pub(super) fn is_valid(&self) -> bool {
        !self.connection.is_null() && unsafe { (self.connection_has_error)(self.connection) } <= 0
    }

    /// Calls `xcb_flush`. This should generally be done at the end of any function in imp.rs, or in any other
    /// situation where a function that was just called needs to be fully completed before moving on.
    pub(super) fn flush(&self) -> Result<(), Error> {
        unsafe {
            let r = (self.flush)(self.connection);
            if r > 0 { Ok(()) } else { Err(Error(r)) }
        }
    }

    /// Calls `xcb_generate_id`. Generating an ID is required to create anything which needs an ID, such as a window.
    pub(super) fn generate_id(&self) -> u32 {
        unsafe { (self.generate_id)(self.connection) }
    }

    /// Calls `xcb_create_window_checked` with the given parameters.
    pub(super) fn create_window(&self, id: XcbWindow, x: i16, y: i16, width: u16, height: u16, border_width: u16, value_mask: u32, value_list: &[u32]) -> Result<(), Error> {
        unsafe {
            let cookie = (self.create_window)(self.connection, 0, id, (*self.screen).root, x, y, width, height, border_width, XCB_WINDOW_CLASS_INPUT_OUTPUT, 0, value_mask, value_list.as_ptr().cast());
            let r = (self.request_check)(self.connection, cookie);
            if r.is_null() {
                Ok(())
            } else {
                Err(Error((*r).error_code.into()))
            }
        }
    }

    /// Calls `xcb_map_window_checked` on the given window.
    pub(super) fn map_window(&self, window: XcbWindow) -> Result<(), Error> {
        unsafe {
            let cookie = (self.map_window)(self.connection, window);
            let r = (self.request_check)(self.connection, cookie);
            if r.is_null() {
                Ok(())
            } else {
                Err(Error((*r).error_code.into()))
            }
        }
    }
}

/// Pointer to dynamically-loaded libxcb.so
struct LibXcb (*mut ffi::c_void);
impl LibXcb {
    fn is_valid(&self) -> bool {
        !self.0.is_null()
    }
}
impl Drop for LibXcb {
    fn drop(&mut self) {
        unsafe { let _ = libc::dlclose(self.0); }
    }
}
unsafe impl Send for LibXcb {}
unsafe impl Sync for LibXcb {}

#[repr(C)]
#[derive(Debug)]
struct ScreenIterator {
    data: *mut Screen,
    rem: raw::c_int,
    index: raw::c_int,
}

#[repr(C)]
#[derive(Debug)]
struct Screen {
    root: XcbWindow,
    default_colourmap: XcbColourMap,
    white_pixel: u32,
    black_pixel: u32,
    current_input_masks: u32,
    width_in_pixels: u16,
    height_in_pixels: u16,
    width_in_millimeters: u16,
    height_in_millimeters: u16,
    min_installed_maps: u16,
    max_installed_maps: u16,
    root_visual: XcbVisualId,
    backing_stores: u8,
    save_unders: u8,
    root_depth: u8,
    allowed_depths_len: u8,
}

unsafe fn setup() -> Xcb {
    // Check validity of our connection to libxcb.so and existence of functions we actually need here
    if !LIBXCB.is_valid() { return Xcb::invalid() }
    let xcb_connect = libc::dlsym(LIBXCB.0, c_string!("xcb_connect"));
    if xcb_connect.is_null() { return Xcb::invalid() }
    let xcb_connection_has_error = libc::dlsym(LIBXCB.0, c_string!("xcb_connection_has_error"));
    if xcb_connection_has_error.is_null() { return Xcb::invalid() }
    let xcb_get_setup = libc::dlsym(LIBXCB.0, c_string!("xcb_get_setup"));
    if xcb_get_setup.is_null() { return Xcb::invalid() }
    let xcb_setup_roots_iterator = libc::dlsym(LIBXCB.0, c_string!("xcb_setup_roots_iterator"));
    if xcb_setup_roots_iterator.is_null() { return Xcb::invalid() }
    let xcb_setup_roots_length = libc::dlsym(LIBXCB.0, c_string!("xcb_setup_roots_length"));
    if xcb_setup_roots_length.is_null() { return Xcb::invalid() }

    // Create an XCB connection
    let xcb_connect: unsafe extern "C" fn(*const raw::c_char, *mut raw::c_int) -> *mut ConnectionPtr = transmute(xcb_connect);
    let xcb_connection_has_error: unsafe extern "C" fn(*mut ConnectionPtr) -> raw::c_int = transmute(xcb_connection_has_error);
    let connection = xcb_connect(ptr::null(), ptr::null_mut());

    // Iterate screens
    enum SetupPtr {}
    let xcb_get_setup: unsafe extern "C" fn(*mut ConnectionPtr) -> *mut SetupPtr = transmute(xcb_get_setup);
    let xcb_setup_roots_iterator: unsafe extern "C" fn(*const SetupPtr) -> ScreenIterator = transmute(xcb_setup_roots_iterator);
    let xcb_setup_roots_length: unsafe extern "C" fn(*const SetupPtr) -> raw::c_int = transmute(xcb_setup_roots_length);
    let setup = xcb_get_setup(connection);
    let length = xcb_setup_roots_length(setup);
    if length <= 0 { return Xcb::invalid() }
    let iter: ScreenIterator = xcb_setup_roots_iterator(setup);
    let screen = iter.data;
    if screen.is_null() { return Xcb::invalid() }

    // Define other functions we'll need
    let request_check = libc::dlsym(LIBXCB.0, c_string!("xcb_request_check"));
    if request_check.is_null() { return Xcb::invalid() }
    let request_check: unsafe extern "C" fn(*mut ConnectionPtr, Cookie) -> *mut XcbGenericError = transmute(request_check);
    let disconnect = libc::dlsym(LIBXCB.0, c_string!("xcb_disconnect"));
    if disconnect.is_null() { return Xcb::invalid() }
    let disconnect: unsafe extern "C" fn(*mut ConnectionPtr) = transmute(disconnect);
    let flush = libc::dlsym(LIBXCB.0, c_string!("xcb_flush"));
    if flush.is_null() { return Xcb::invalid() }
    let flush: unsafe extern "C" fn(*mut ConnectionPtr) -> raw::c_int = transmute(flush);
    let generate_id = libc::dlsym(LIBXCB.0, c_string!("xcb_generate_id"));
    if generate_id.is_null() { return Xcb::invalid() }
    let generate_id: unsafe extern "C" fn(*mut ConnectionPtr) -> u32 = transmute(generate_id);
    let create_window = libc::dlsym(LIBXCB.0, c_string!("xcb_create_window_checked"));
    if create_window.is_null() { return Xcb::invalid() }
    let create_window: unsafe extern "C" fn(*mut ConnectionPtr, u8, XcbWindow, XcbWindow, i16, i16, u16, u16, u16, u16, XcbVisualId, u32, *const ffi::c_void) -> Cookie = transmute(create_window);
    let map_window = libc::dlsym(LIBXCB.0, c_string!("xcb_map_window_checked"));
    if map_window.is_null() { return Xcb::invalid() }
    let map_window: unsafe extern "C" fn(*mut ConnectionPtr, XcbWindow) -> Cookie = transmute(map_window);

    let err = xcb_connection_has_error(connection);
    if  err <= 0 {
        Xcb {
            connection,
            screen,
            request_check,
            connection_has_error: xcb_connection_has_error,
            disconnect,
            flush,
            generate_id,
            create_window,
            map_window,
        }
    } else {
        Xcb::invalid()
    }
}

lazy_static::lazy_static! {
    static ref LIBXCB: LibXcb = LibXcb(unsafe { libc::dlopen(c_string!("libxcb.so.1"), libc::RTLD_LOCAL | libc::RTLD_LAZY) });
    pub(super) static ref XCB: Xcb = unsafe { setup() };
}