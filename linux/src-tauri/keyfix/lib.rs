//! Hides `zwp_text_input_manager_v3` from Wine.
//!
//! Wine enables that protocol for the whole window. KWin then hands printable
//! keys to the input method, so chat receives characters and the game never
//! sees WASD as a key press. Letters still arrive through the XKB layout Wine
//! builds on Wayland.

use std::ffi::{CStr, c_char, c_int, c_void};
use std::mem;
use std::sync::OnceLock;

const RTLD_NEXT: *mut c_void = -1isize as *mut c_void;

type GlobalFn = unsafe extern "C" fn(*mut c_void, *mut c_void, u32, *const c_char, u32);
type RemoveFn = unsafe extern "C" fn(*mut c_void, *mut c_void, u32);
type AddListenerFn = unsafe extern "C" fn(*mut c_void, *const *const c_void, *mut c_void) -> c_int;

#[link(name = "dl")]
unsafe extern "C" {
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

#[link(name = "wayland-client")]
unsafe extern "C" {
    fn wl_proxy_get_class(proxy: *mut c_void) -> *const c_char;
}

#[repr(C)]
struct RegistryListener {
    global: unsafe extern "C" fn(*mut c_void, *mut c_void, u32, *const c_char, u32),
    global_remove: unsafe extern "C" fn(*mut c_void, *mut c_void, u32),
}

struct Hook {
    global: GlobalFn,
    remove: Option<RemoveFn>,
    data: *mut c_void,
}

unsafe impl Send for Hook {}
unsafe impl Sync for Hook {}

static LISTENER: RegistryListener = RegistryListener {
    global: our_global,
    global_remove: our_remove,
};

fn hide_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("VORTEX_HIDE_TEXT_INPUT").as_deref() == Some("1".as_ref()))
}

fn is_hidden_interface(name: &[u8]) -> bool {
    name == b"zwp_text_input_manager_v3"
}

fn real_add_listener() -> AddListenerFn {
    static REAL: OnceLock<AddListenerFn> = OnceLock::new();
    *REAL.get_or_init(|| {
        let name = c"wl_proxy_add_listener";
        let sym = unsafe { dlsym(RTLD_NEXT, name.as_ptr()) };
        assert!(!sym.is_null(), "wl_proxy_add_listener");
        unsafe { mem::transmute(sym) }
    })
}

unsafe extern "C" fn our_global(
    data: *mut c_void,
    registry: *mut c_void,
    name: u32,
    interface: *const c_char,
    version: u32,
) {
    let hook = unsafe { &*(data as *const Hook) };
    if !interface.is_null() {
        let bytes = unsafe { CStr::from_ptr(interface) }.to_bytes();
        if is_hidden_interface(bytes) {
            return;
        }
    }
    unsafe { (hook.global)(hook.data, registry, name, interface, version) };
}

unsafe extern "C" fn our_remove(data: *mut c_void, registry: *mut c_void, name: u32) {
    let hook = unsafe { &*(data as *const Hook) };
    if let Some(remove) = hook.remove {
        unsafe { remove(hook.data, registry, name) };
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wl_proxy_add_listener(
    proxy: *mut c_void,
    implementation: *const *const c_void,
    data: *mut c_void,
) -> c_int {
    let real = real_add_listener();
    if !hide_enabled() || proxy.is_null() || implementation.is_null() {
        return unsafe { real(proxy, implementation, data) };
    }

    let class = unsafe { wl_proxy_get_class(proxy) };
    let is_registry = !class.is_null() && unsafe { CStr::from_ptr(class) }.to_bytes() == b"wl_registry";
    if !is_registry {
        return unsafe { real(proxy, implementation, data) };
    }

    let global_ptr = unsafe { *implementation };
    if global_ptr.is_null() {
        return unsafe { real(proxy, implementation, data) };
    }
    let remove_ptr = unsafe { *implementation.add(1) };
    let hook = Box::leak(Box::new(Hook {
        global: unsafe { mem::transmute(global_ptr) },
        remove: if remove_ptr.is_null() {
            None
        } else {
            Some(unsafe { mem::transmute(remove_ptr) })
        },
        data,
    }));
    let listener = &LISTENER as *const RegistryListener as *const *const c_void;
    unsafe { real(proxy, listener, hook as *mut Hook as *mut c_void) }
}

#[cfg(test)]
mod tests {
    use super::is_hidden_interface;

    #[test]
    fn hides_only_text_input_v3() {
        assert!(is_hidden_interface(b"zwp_text_input_manager_v3"));
        assert!(!is_hidden_interface(b"wl_seat"));
        assert!(!is_hidden_interface(b"zwp_text_input_manager_v2"));
    }
}
