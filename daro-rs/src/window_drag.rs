//! Windows 下自绘标题栏的拖动与缩放兜底。
//!
//! gpui 的 WM_NCHITTEST 依据上一次鼠标命中的缓存判断是否返回 HTCAPTION，
//! 而窗口失活时不会更新这份缓存，于是首次点击标题栏被当作客户区点击，
//! 系统不会启动移动循环。这里在这种情形下直接发 SC_DRAGMOVE，
//! 进系统的标题栏拖动。

/// 最大化 / 还原切换。
///
/// 不能用 `window.zoom_window()`：gpui 在 Windows 上把它实现成只发
/// `SW_MAXIMIZE`（gpui-pre-windows `zoom()`），已最大化时再双击没有任何反应。
/// 另外 gpui 会把 `WM_NCLBUTTONDBLCLK` 也当事件派发进元素树，我们
/// `stop_propagation` 之后系统的默认切换就不会发生，所以还原得自己补。
pub fn toggle_zoom(window: &gpui_kit::Window) {
    #[cfg(target_os = "windows")]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        use windows_sys::Win32::UI::WindowsAndMessaging::{ShowWindowAsync, SW_RESTORE};

        if !window.is_maximized() {
            window.zoom_window();
            return;
        }
        let Ok(handle) = <gpui_kit::Window as HasWindowHandle>::window_handle(window) else {
            return;
        };
        let RawWindowHandle::Win32(win32) = handle.as_raw() else {
            return;
        };
        unsafe {
            // 返回值只是「请求是否送达窗口线程」，最大化状态由系统回调同步。
            let _ = ShowWindowAsync(win32.hwnd.get() as *mut core::ffi::c_void, SW_RESTORE);
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        // macOS 的 zoom: 本身就是最大化 / 还原切换，Linux 交给窗管同名语义。
        window.zoom_window();
    }
}

#[cfg(target_os = "windows")]
pub fn begin_window_drag(window: &gpui_kit::Window) -> bool {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetCapture, ReleaseCapture};
    use windows_sys::Win32::UI::WindowsAndMessaging::{SendMessageW, WM_SYSCOMMAND};

    // 0xF010 | 2：HTCAPTION 对应的 SC_NORESIZE+2，未命名进 windows-sys。
    const SC_DRAGMOVE: usize = 0xF012;

    let Ok(handle) = <gpui_kit::Window as HasWindowHandle>::window_handle(window) else {
        return false;
    };
    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        return false;
    };
    let hwnd = win32.hwnd.get() as *mut core::ffi::c_void;
    // 非客户区路径由 gpui 交给 DefWindowProc，这里不接管；
    // 只有客户区路径（gpui 已 SetCapture）才需要补发。
    if unsafe { GetCapture() } != hwnd {
        return false;
    }
    // 原本靠补发 WM_NCLBUTTONDOWN 进系统移动循环，但那是一次完整的鼠标消息：
    // gpui 会把它派发进元素树并计入自己的双击计数（issue #28 的回弹就这么来的），
    // Windows 还会把它当成标题栏第一下去喂双击→最大化判定。SC_DRAGMOVE 直接进
    // 移动循环，两条污染都没有；DefWindowProc 不再顺带激活窗口，激活自己补。
    window.activate_window();
    unsafe {
        ReleaseCapture();
        let _ = SendMessageW(hwnd, WM_SYSCOMMAND, SC_DRAGMOVE, 0);
    }
    true
}

#[cfg(not(target_os = "windows"))]
pub fn begin_window_drag(_window: &gpui_kit::Window) -> bool {
    false
}
