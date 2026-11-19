//! Pure Linux / X11 window & process focus utilities:
//!   - `/proc` parent-PID graph for terminal window matching & seen-clears-it
//!   - Dynamic `libX11.so.6` EWMH helpers (`_NET_ACTIVE_WINDOW`, `_NET_CLIENT_LIST`,
//!     `_NET_WM_PID`, `_NET_WM_STATE` / `_NET_WM_STATE_FULLSCREEN`)
//!   - `XQueryPointer` left-button drag query for edge-carrying the notch

use std::collections::HashMap;

pub struct ProcMaps {
    pub ppid: HashMap<u32, u32>,
    pub name: HashMap<u32, String>,
}

pub fn chain_of(pid: u32, ppid: &HashMap<u32, u32>) -> Vec<u32> {
    let mut chain = vec![pid];
    let mut cur = pid;
    for _ in 0..8 {
        match ppid.get(&cur) {
            Some(&p) if p != 0 && !chain.contains(&p) => {
                chain.push(p);
                cur = p;
            }
            _ => break,
        }
    }
    chain
}

pub fn pid_hits_chain(pid: u32, chain: &[u32], maps: &ProcMaps) -> bool {
    chain.contains(&pid)
        || maps
            .ppid
            .get(&pid)
            .map(|p| chain.contains(p))
            .unwrap_or(false)
}

pub mod x11_helpers {
    pub type Display = core::ffi::c_void;
    pub type Window = core::ffi::c_ulong;
    pub type Atom = core::ffi::c_ulong;
    pub type Bool = core::ffi::c_int;

    #[repr(C)]
    pub union ClientData {
        pub b: [i8; 20],
        pub s: [i16; 10],
        pub l: [core::ffi::c_long; 5],
    }

    #[repr(C)]
    pub struct XClientMessageEvent {
        pub type_: core::ffi::c_int,
        pub serial: core::ffi::c_ulong,
        pub send_event: Bool,
        pub display: *mut Display,
        pub window: Window,
        pub message_type: Atom,
        pub format: core::ffi::c_int,
        pub data: ClientData,
    }

    #[repr(C)]
    pub union XEvent {
        pub type_: core::ffi::c_int,
        pub xclient: std::mem::ManuallyDrop<XClientMessageEvent>,
        pub pad: [core::ffi::c_long; 24],
    }

    pub struct X11 {
        #[allow(dead_code)]
        pub lib: *mut core::ffi::c_void,
        pub open_display: unsafe extern "C" fn(*const i8) -> *mut Display,
        pub close_display: unsafe extern "C" fn(*mut Display) -> core::ffi::c_int,
        pub default_root_window: unsafe extern "C" fn(*mut Display) -> Window,
        pub intern_atom: unsafe extern "C" fn(*mut Display, *const i8, Bool) -> Atom,
        pub get_window_property: unsafe extern "C" fn(
            *mut Display,
            Window,
            Atom,
            core::ffi::c_long,
            core::ffi::c_long,
            Bool,
            Atom,
            *mut Atom,
            *mut core::ffi::c_int,
            *mut core::ffi::c_ulong,
            *mut core::ffi::c_ulong,
            *mut *mut u8,
        ) -> core::ffi::c_int,
        pub free: unsafe extern "C" fn(*mut core::ffi::c_void) -> core::ffi::c_int,
        pub send_event:
            unsafe extern "C" fn(*mut Display, Window, Bool, core::ffi::c_long, *mut XEvent) -> core::ffi::c_int,
        pub map_raised: unsafe extern "C" fn(*mut Display, Window) -> core::ffi::c_int,
        pub flush: unsafe extern "C" fn(*mut Display) -> core::ffi::c_int,
        pub query_pointer: unsafe extern "C" fn(
            *mut Display,
            Window,
            *mut Window,
            *mut Window,
            *mut i32,
            *mut i32,
            *mut i32,
            *mut i32,
            *mut u32,
        ) -> Bool,
    }

    unsafe impl Send for X11 {}
    unsafe impl Sync for X11 {}

    extern "C" {
        fn dlopen(filename: *const std::os::raw::c_char, flag: std::os::raw::c_int) -> *mut core::ffi::c_void;
        fn dlsym(handle: *mut core::ffi::c_void, symbol: *const std::os::raw::c_char) -> *mut core::ffi::c_void;
    }

    pub fn load_x11() -> Option<&'static X11> {
        static INSTANCE: std::sync::OnceLock<Option<X11>> = std::sync::OnceLock::new();
        INSTANCE
            .get_or_init(|| unsafe {
                let lib = dlopen(b"libX11.so.6\0".as_ptr().cast(), 1 /* RTLD_LAZY */);
                if lib.is_null() {
                    return None;
                }
                macro_rules! sym {
                    ($name:expr) => {{
                        let ptr = dlsym(lib, concat!($name, "\0").as_ptr().cast());
                        if ptr.is_null() {
                            return None;
                        }
                        core::mem::transmute(ptr)
                    }};
                }
                Some(X11 {
                    lib,
                    open_display: sym!("XOpenDisplay"),
                    close_display: sym!("XCloseDisplay"),
                    default_root_window: sym!("XDefaultRootWindow"),
                    intern_atom: sym!("XInternAtom"),
                    get_window_property: sym!("XGetWindowProperty"),
                    free: sym!("XFree"),
                    send_event: sym!("XSendEvent"),
                    map_raised: sym!("XMapRaised"),
                    flush: sym!("XFlush"),
                    query_pointer: sym!("XQueryPointer"),
                })
            })
            .as_ref()
    }

    pub unsafe fn get_window_prop_ulong(
        x11: &X11,
        dpy: *mut Display,
        w: Window,
        atom: Atom,
    ) -> Option<core::ffi::c_ulong> {
        let mut actual_type: Atom = 0;
        let mut actual_format: core::ffi::c_int = 0;
        let mut nitems: core::ffi::c_ulong = 0;
        let mut bytes_after: core::ffi::c_ulong = 0;
        let mut prop: *mut u8 = core::ptr::null_mut();

        let st = (x11.get_window_property)(
            dpy,
            w,
            atom,
            0,
            1,
            0,
            0,
            &mut actual_type,
            &mut actual_format,
            &mut nitems,
            &mut bytes_after,
            &mut prop,
        );

        if st == 0 && !prop.is_null() && nitems > 0 {
            let val = *(prop as *const core::ffi::c_ulong);
            (x11.free)(prop.cast());
            Some(val)
        } else {
            if !prop.is_null() {
                (x11.free)(prop.cast());
            }
            None
        }
    }

    pub unsafe fn get_window_prop_ulong_list(
        x11: &X11,
        dpy: *mut Display,
        w: Window,
        atom: Atom,
    ) -> Vec<core::ffi::c_ulong> {
        let mut actual_type: Atom = 0;
        let mut actual_format: core::ffi::c_int = 0;
        let mut nitems: core::ffi::c_ulong = 0;
        let mut bytes_after: core::ffi::c_ulong = 0;
        let mut prop: *mut u8 = core::ptr::null_mut();

        let st = (x11.get_window_property)(
            dpy,
            w,
            atom,
            0,
            1024,
            0,
            0,
            &mut actual_type,
            &mut actual_format,
            &mut nitems,
            &mut bytes_after,
            &mut prop,
        );

        let mut out = Vec::new();
        if st == 0 && !prop.is_null() && nitems > 0 {
            let slice = std::slice::from_raw_parts(prop as *const core::ffi::c_ulong, nitems as usize);
            out.extend_from_slice(slice);
            (x11.free)(prop.cast());
        } else if !prop.is_null() {
            (x11.free)(prop.cast());
        }
        out
    }

    pub unsafe fn activate_window(x11: &X11, dpy: *mut Display, root: Window, target: Window) {
        let net_active_window = (x11.intern_atom)(dpy, b"_NET_ACTIVE_WINDOW\0".as_ptr().cast(), 0);
        let mut ev: XEvent = core::mem::zeroed();
        ev.type_ = 33; // ClientMessage
        let xc = &mut ev.xclient;
        xc.window = target;
        xc.message_type = net_active_window;
        xc.format = 32;
        xc.data.l[0] = 1; // 1 = application
        xc.data.l[1] = 0; // CurrentTime

        const SUBSTRUCTURE_NOTIFY: core::ffi::c_long = 1 << 19;
        const SUBSTRUCTURE_REDIRECT: core::ffi::c_long = 1 << 20;

        (x11.send_event)(dpy, root, 0, SUBSTRUCTURE_NOTIFY | SUBSTRUCTURE_REDIRECT, &mut ev);
        (x11.map_raised)(dpy, target);
        (x11.flush)(dpy);
    }
}

pub fn proc_maps() -> ProcMaps {
    let mut ppid_map = HashMap::new();
    let mut name_map = HashMap::new();

    if let Ok(entries) = std::fs::read_dir("/proc") {
        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let Some(name_str) = file_name.to_str() else { continue };
            let Ok(pid) = name_str.parse::<u32>() else { continue };

            let stat_path = entry.path().join("stat");
            let Ok(content) = std::fs::read_to_string(stat_path) else { continue };

            let Some(lparen) = content.find('(') else { continue };
            let Some(rparen) = content.rfind(')') else { continue };
            if lparen >= rparen {
                continue;
            }

            let comm = content[lparen + 1..rparen].to_lowercase();
            let rest = content[rparen + 1..].trim_start();
            let mut parts = rest.split_whitespace();
            let _state = parts.next();
            if let Some(ppid_str) = parts.next() {
                if let Ok(ppid) = ppid_str.parse::<u32>() {
                    ppid_map.insert(pid, ppid);
                    name_map.insert(pid, comm);
                }
            }
        }
    }

    ProcMaps {
        ppid: ppid_map,
        name: name_map,
    }
}

pub fn fg_pid() -> u32 {
    let Some(x11) = x11_helpers::load_x11() else { return 0 };
    unsafe {
        let dpy = (x11.open_display)(core::ptr::null());
        if dpy.is_null() {
            return 0;
        }
        let root = (x11.default_root_window)(dpy);
        let net_active_window = (x11.intern_atom)(dpy, b"_NET_ACTIVE_WINDOW\0".as_ptr().cast(), 0);
        let net_wm_pid = (x11.intern_atom)(dpy, b"_NET_WM_PID\0".as_ptr().cast(), 0);

        let mut pid = 0u32;
        if let Some(active_win) = x11_helpers::get_window_prop_ulong(x11, dpy, root, net_active_window) {
            if active_win != 0 {
                if let Some(win_pid) = x11_helpers::get_window_prop_ulong(x11, dpy, active_win, net_wm_pid) {
                    pid = win_pid as u32;
                }
            }
        }
        (x11.close_display)(dpy);
        pid
    }
}

/// Checks whether the active X11 window carries EWMH `_NET_WM_STATE_FULLSCREEN`
/// (and does not belong to Codenotch itself).
pub fn active_window_is_fullscreen() -> Option<bool> {
    let x11 = x11_helpers::load_x11()?;
    let my_pid = std::process::id();
    unsafe {
        let dpy = (x11.open_display)(core::ptr::null());
        if dpy.is_null() {
            return None;
        }
        let root = (x11.default_root_window)(dpy);
        let net_active = (x11.intern_atom)(dpy, b"_NET_ACTIVE_WINDOW\0".as_ptr().cast(), 0);
        let net_wm_pid = (x11.intern_atom)(dpy, b"_NET_WM_PID\0".as_ptr().cast(), 0);
        let net_wm_state = (x11.intern_atom)(dpy, b"_NET_WM_STATE\0".as_ptr().cast(), 0);
        let net_wm_fullscreen = (x11.intern_atom)(dpy, b"_NET_WM_STATE_FULLSCREEN\0".as_ptr().cast(), 0);

        let result = match x11_helpers::get_window_prop_ulong(x11, dpy, root, net_active) {
            Some(win) if win != 0 => {
                let win_pid = x11_helpers::get_window_prop_ulong(x11, dpy, win, net_wm_pid).unwrap_or(0) as u32;
                if win_pid == my_pid {
                    None
                } else {
                    let states = x11_helpers::get_window_prop_ulong_list(x11, dpy, win, net_wm_state);
                    Some(states.contains(&net_wm_fullscreen))
                }
            }
            _ => Some(false),
        };
        (x11.close_display)(dpy);
        result
    }
}

pub fn focus_terminal(claude_pid: u32) -> bool {
    if claude_pid == 0 {
        return false;
    }
    let maps = proc_maps();
    let chain = chain_of(claude_pid, &maps.ppid);

    let Some(x11) = x11_helpers::load_x11() else { return false };
    unsafe {
        let dpy = (x11.open_display)(core::ptr::null());
        if dpy.is_null() {
            return false;
        }
        let root = (x11.default_root_window)(dpy);
        let net_client_list = (x11.intern_atom)(dpy, b"_NET_CLIENT_LIST\0".as_ptr().cast(), 0);
        let net_wm_pid = (x11.intern_atom)(dpy, b"_NET_WM_PID\0".as_ptr().cast(), 0);

        let clients = x11_helpers::get_window_prop_ulong_list(x11, dpy, root, net_client_list);

        let score_of = |pid: u32| -> Option<usize> {
            if let Some(i) = chain.iter().position(|&c| c == pid) {
                return Some(i);
            }
            if let Some(&pp) = maps.ppid.get(&pid) {
                if let Some(i) = chain.iter().position(|&c| c == pp) {
                    return Some(i);
                }
            }
            None
        };

        let mut best: Option<(usize, x11_helpers::Window)> = None;
        for win in clients {
            if let Some(win_pid) = x11_helpers::get_window_prop_ulong(x11, dpy, win, net_wm_pid) {
                if let Some(score) = score_of(win_pid as u32) {
                    if best.map(|(s, _)| score > s).unwrap_or(true) {
                        best = Some((score, win));
                    }
                }
            }
        }

        let success = if let Some((_, target_win)) = best {
            x11_helpers::activate_window(x11, dpy, root, target_win);
            true
        } else {
            false
        };

        (x11.close_display)(dpy);
        success
    }
}

pub fn focus_claude_desktop() -> bool {
    let maps = proc_maps();
    let Some(x11) = x11_helpers::load_x11() else { return false };
    unsafe {
        let dpy = (x11.open_display)(core::ptr::null());
        if dpy.is_null() {
            return false;
        }
        let root = (x11.default_root_window)(dpy);
        let net_client_list = (x11.intern_atom)(dpy, b"_NET_CLIENT_LIST\0".as_ptr().cast(), 0);
        let net_wm_pid = (x11.intern_atom)(dpy, b"_NET_WM_PID\0".as_ptr().cast(), 0);

        let clients = x11_helpers::get_window_prop_ulong_list(x11, dpy, root, net_client_list);
        let mut target = None;
        for win in clients {
            if let Some(win_pid) = x11_helpers::get_window_prop_ulong(x11, dpy, win, net_wm_pid) {
                if let Some(name) = maps.name.get(&(win_pid as u32)) {
                    if name.contains("claude") && !name.contains("codenotch") {
                        target = Some(win);
                        break;
                    }
                }
            }
        }

        let success = if let Some(win) = target {
            x11_helpers::activate_window(x11, dpy, root, win);
            true
        } else {
            false
        };

        (x11.close_display)(dpy);
        success
    }
}

pub fn query_left_button_down() -> bool {
    let Some(x11) = x11_helpers::load_x11() else { return false };
    thread_local! {
        static DPY: std::cell::Cell<*mut x11_helpers::Display> = const { std::cell::Cell::new(core::ptr::null_mut()) };
    }
    DPY.with(|d| {
        let mut dpy = d.get();
        if dpy.is_null() {
            unsafe {
                dpy = (x11.open_display)(core::ptr::null());
                d.set(dpy);
            }
        }
        if dpy.is_null() {
            return false;
        }

        unsafe {
            let root = (x11.default_root_window)(dpy);
            let mut root_return: x11_helpers::Window = 0;
            let mut child_return: x11_helpers::Window = 0;
            let mut root_x: i32 = 0;
            let mut root_y: i32 = 0;
            let mut win_x: i32 = 0;
            let mut win_y: i32 = 0;
            let mut mask: u32 = 0;

            let ok = (x11.query_pointer)(
                dpy,
                root,
                &mut root_return,
                &mut child_return,
                &mut root_x,
                &mut root_y,
                &mut win_x,
                &mut win_y,
                &mut mask,
            );

            if ok == 0 {
                (x11.close_display)(dpy);
                d.set(core::ptr::null_mut());
                return false;
            }

            const BUTTON1_MASK: u32 = 1 << 8;
            (mask & BUTTON1_MASK) != 0
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walks_proc_ancestor_chain_without_cycles() {
        let mut ppid = HashMap::new();
        ppid.insert(300, 200);
        ppid.insert(200, 100);
        ppid.insert(100, 1);
        assert_eq!(chain_of(300, &ppid), vec![300, 200, 100, 1]);
    }

    #[test]
    fn proc_maps_finds_current_process() {
        let maps = proc_maps();
        assert!(maps.ppid.contains_key(&std::process::id()));
    }
}
