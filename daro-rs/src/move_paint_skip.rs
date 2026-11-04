//! Windows 下拖动/缩放窗口时跳过移动循环里的整帧重绘，消除拖动跳格。
//!
//! gpui 的移动/缩放循环（WM_ENTERSIZEMOVE）期间画面更新有两条路：它自己 10ms 的
//! WM_TIMER（gpui-fast-windows events.rs: SIZE_MOVE_LOOP_TIMER_ID = 1）里同步
//! draw_window，以及队列空闲时插进来的 WM_PAINT。本应用一帧要画的元素极多
//! （数据页一次画完整页上千行 × 所有列），模态移动循环被这两条路一次次堵住，
//! 光标 65Hz 移动时窗口位置只跟到刷新得过来的频率，拖起来一卡一卡。
//!
//! 而纯移动期间窗口内容不会变——DWM 会把整块表面跟着窗口平移，重绘纯属浪费。
//! 这里挂一层 subclass：进入循环时记下窗口大小，循环期间凡是「纯移动」（大小没变）
//! 就把 id=1 的 WM_TIMER 吞掉、WM_PAINT 只清更新区不重绘；大小一旦变了（拖边框
//! 缩放、跨屏 DPI 变化、贴边最大化）照常放行，gpui 的重排重绘不受影响。
//! WM_PAINT 必须先 ValidateRect 再返回 0，否则更新区不清空会无限重入。
//! 移动结束后恢复常规节奏，拖动期间攒下的任务由 gpui 的常规循环补画。

#[cfg(target_os = "windows")]
mod imp {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::Foundation::{LPARAM, LRESULT, RECT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::ValidateRect;
    use windows_sys::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowRect, WM_ENTERSIZEMOVE, WM_EXITSIZEMOVE, WM_PAINT, WM_TIMER,
    };

    /// gpui-fast-windows 里移动循环定时器的 ID。
    const GPUI_SIZE_MOVE_TIMER_ID: usize = 1;

    thread_local! {
        /// hwnd -> 进入移动/缩放循环时的窗口外框 (宽, 高)；空表 = 不在循环里
        static ENTER_SIZE: RefCell<HashMap<isize, (i32, i32)>> = RefCell::new(HashMap::new());
    }

    fn outer_size(hwnd: windows_sys::Win32::Foundation::HWND) -> Option<(i32, i32)> {
        let mut r = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        if unsafe { GetWindowRect(hwnd, &mut r) } == 0 {
            return None;
        }
        Some((r.right - r.left, r.bottom - r.top))
    }

    /// 是否处于「纯移动」：在移动/缩放循环里，且窗口外框与进入时一致。
    fn pure_move(hwnd: windows_sys::Win32::Foundation::HWND) -> bool {
        ENTER_SIZE.with(|m| m.borrow().get(&(hwnd as isize)).copied()) == outer_size(hwnd)
    }

    unsafe extern "system" fn drag_pump_proc(
        hwnd: windows_sys::Win32::Foundation::HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _id: usize,
        _data: usize,
    ) -> LRESULT {
        match msg {
            WM_ENTERSIZEMOVE => {
                if let Some(size) = outer_size(hwnd) {
                    ENTER_SIZE.with(|m| m.borrow_mut().insert(hwnd as isize, size));
                }
            }
            WM_EXITSIZEMOVE => {
                ENTER_SIZE.with(|m| m.borrow_mut().remove(&(hwnd as isize)));
            }
            WM_TIMER if wparam == GPUI_SIZE_MOVE_TIMER_ID && pure_move(hwnd) => return 0,
            WM_PAINT if pure_move(hwnd) => {
                unsafe { ValidateRect(hwnd, core::ptr::null_mut()) };
                return 0;
            }
            _ => {}
        }
        DefSubclassProc(hwnd, msg, wparam, lparam)
    }

    /// 主窗口创建后调用一次；重复调用对同一个 subclass id 会替换旧的，无副作用。
    pub fn install(window: &gpui_kit::Window) {
        let Ok(handle) = <gpui_kit::Window as HasWindowHandle>::window_handle(window) else {
            return;
        };
        let RawWindowHandle::Win32(win32) = handle.as_raw() else {
            return;
        };
        let hwnd = win32.hwnd.get() as windows_sys::Win32::Foundation::HWND;
        unsafe {
            SetWindowSubclass(hwnd, Some(drag_pump_proc), 0x4452, 0);
        }
    }
}

#[cfg(target_os = "windows")]
pub use imp::install;

#[cfg(not(target_os = "windows"))]
pub fn install(_window: &gpui_kit::Window) {}
