//! 截图工具（微信 Alt+A 等）的「自动分块」按窗口层次取块：参考界面这类
//! VCL 程序每个面板都是一个真实子 HWND，而 GPUI 整个应用只有一张顶层窗口，
//! 截图工具于是只能识别到整窗。这里把主界面的几个大区块登记成子窗口，
//! 位置随布局同步；子窗口不接收输入（WM_NCHITTEST 返回 HTTRANSPARENT）、
//! 不参与绘制，只负责被枚举起一块矩形和名字。

/// 区块名与客户区矩形（逻辑像素，原点在客户区左上角）。
pub struct Region<'a> {
    pub name: &'a str,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// 把 `regions` 同步成 `window` 的子窗口；每帧调用，只搬变化过的。
#[cfg(target_os = "windows")]
pub fn sync(window: &gpui_kit::Window, regions: &[Region<'_>]) {
    imp::sync(window, regions)
}

#[cfg(not(target_os = "windows"))]
pub fn sync(_window: &gpui_kit::Window, _regions: &[Region<'_>]) {}

#[cfg(target_os = "windows")]
mod imp {
    use super::Region;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use std::cell::RefCell;
    use std::sync::{Once, OnceLock};
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::ValidateRect;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, IsWindow, RegisterClassExW, SetWindowPos,
        SetWindowTextW, MA_NOACTIVATE, SWP_NOACTIVATE, SWP_NOZORDER, WM_ERASEBKGND,
        WM_MOUSEACTIVATE, WM_NCHITTEST, WM_PAINT, WM_PRINTCLIENT, WNDCLASSEXW, WS_CHILD,
        WS_CLIPSIBLINGS, WS_EX_NOACTIVATE, WS_VISIBLE,
    };

    const CLASS: &str = "DaroBlockRegion";

    struct Slot {
        hwnd: HWND,
        name: String,
        rect: (i32, i32, i32, i32),
    }

    thread_local! {
        static CHILDREN: RefCell<Vec<Slot>> = const { RefCell::new(Vec::new()) };
        static PARENT: RefCell<isize> = const { RefCell::new(0) };
    }

    pub fn sync(window: &gpui_kit::Window, regions: &[Region<'_>]) {
        let Ok(handle) = <gpui_kit::Window as HasWindowHandle>::window_handle(window) else {
            return;
        };
        let RawWindowHandle::Win32(win32) = handle.as_raw() else {
            return;
        };
        let parent = win32.hwnd.get() as HWND;
        let scale = window.scale_factor();
        if parent.is_null() {
            return;
        }
        PARENT.with(|p| {
            let mut p = p.borrow_mut();
            if *p != (parent as isize) {
                CHILDREN.with(|cs| drop_children(&mut cs.borrow_mut()));
                *p = parent as isize;
            }
        });
        CHILDREN.with(|cs| {
            let mut cs = cs.borrow_mut();
            for (ix, r) in regions.iter().enumerate() {
                let rect = pixel_rect(r, scale);
                match cs.get_mut(ix) {
                    Some(slot) => {
                        if slot.name != r.name {
                            set_text(slot.hwnd, r.name);
                            slot.name.clear();
                            slot.name.push_str(r.name);
                        }
                        if slot.rect != rect {
                            set_rect(slot.hwnd, rect);
                            slot.rect = rect;
                        }
                    }
                    None => {
                        if let Some(hwnd) = create(parent, r.name, rect) {
                            cs.push(Slot {
                                hwnd,
                                name: r.name.to_string(),
                                rect,
                            });
                        }
                    }
                }
            }
            if cs.len() > regions.len() {
                let extra = cs.split_off(regions.len());
                for slot in extra {
                    unsafe { DestroyWindow(slot.hwnd) };
                }
            }
        });
    }

    fn drop_children(cs: &mut Vec<Slot>) {
        for slot in cs.drain(..) {
            unsafe {
                if IsWindow(slot.hwnd) != 0 {
                    DestroyWindow(slot.hwnd);
                }
            }
        }
    }

    fn pixel_rect(r: &Region<'_>, scale: f32) -> (i32, i32, i32, i32) {
        let px = |v: f32| (v * scale).round() as i32;
        (px(r.x), px(r.y), px(r.w).max(1), px(r.h).max(1))
    }

    fn create(parent: HWND, name: &str, (x, y, w, h): (i32, i32, i32, i32)) -> Option<HWND> {
        register_class();
        let text = wide(name);
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_NOACTIVATE,
                class_name().as_ptr(),
                text.as_ptr(),
                WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS,
                x,
                y,
                w,
                h,
                parent,
                std::ptr::null_mut(),
                GetModuleHandleW(std::ptr::null()),
                std::ptr::null(),
            )
        };
        (!hwnd.is_null()).then_some(hwnd)
    }

    fn set_rect(hwnd: HWND, (x, y, w, h): (i32, i32, i32, i32)) {
        unsafe {
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                x,
                y,
                w,
                h,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }

    fn set_text(hwnd: HWND, text: &str) {
        let text = wide(text);
        unsafe { SetWindowTextW(hwnd, text.as_ptr()) };
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn class_name() -> &'static [u16] {
        static NAME: OnceLock<Vec<u16>> = OnceLock::new();
        NAME.get_or_init(|| wide(CLASS)).as_slice()
    }

    fn register_class() {
        static ONCE: Once = Once::new();
        ONCE.call_once(|| {
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: 0,
                lpfnWndProc: Some(wnd_proc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: unsafe { GetModuleHandleW(std::ptr::null()) },
                hIcon: std::ptr::null_mut(),
                hCursor: std::ptr::null_mut(),
                hbrBackground: std::ptr::null_mut(),
                lpszMenuName: std::ptr::null(),
                lpszClassName: class_name().as_ptr(),
                hIconSm: std::ptr::null_mut(),
            };
            unsafe { RegisterClassExW(&wc) };
        });
    }

    unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            // 指针落到分块上等于落在下面的 GPUI 窗口上：命中测试与鼠标消息直接穿透
            WM_NCHITTEST => -1,
            WM_MOUSEACTIVATE => MA_NOACTIVATE as LRESULT,
            WM_ERASEBKGND => 1,
            WM_PAINT => {
                unsafe { ValidateRect(hwnd, std::ptr::null()) };
                0
            }
            WM_PRINTCLIENT => 0,
            _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
        }
    }
}
