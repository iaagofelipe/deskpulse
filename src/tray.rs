use std::path::PathBuf;
use tokio::sync::watch;

#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    OpenPhone,
    OpenAdmin,
    OpenBrowser,
    EditConfig,
    ToggleStartup,
    Quit,
}

#[cfg_attr(not(windows), allow(dead_code))]
pub const MENU_ID_OPEN_PHONE: u32 = 1001;
#[cfg_attr(not(windows), allow(dead_code))]
pub const MENU_ID_OPEN_BROWSER: u32 = 1002;
#[cfg_attr(not(windows), allow(dead_code))]
pub const MENU_ID_EDIT_CONFIG: u32 = 1003;
#[cfg_attr(not(windows), allow(dead_code))]
pub const MENU_ID_TOGGLE_STARTUP: u32 = 1004;
#[cfg_attr(not(windows), allow(dead_code))]
pub const MENU_ID_QUIT: u32 = 1005;
#[cfg_attr(not(windows), allow(dead_code))]
pub const MENU_ID_OPEN_ADMIN: u32 = 1006;

#[cfg_attr(not(windows), allow(dead_code))]
pub fn tray_action_from_menu_id(id: u32) -> Option<TrayAction> {
    match id {
        MENU_ID_OPEN_PHONE => Some(TrayAction::OpenPhone),
        MENU_ID_OPEN_ADMIN => Some(TrayAction::OpenAdmin),
        MENU_ID_OPEN_BROWSER => Some(TrayAction::OpenBrowser),
        MENU_ID_EDIT_CONFIG => Some(TrayAction::EditConfig),
        MENU_ID_TOGGLE_STARTUP => Some(TrayAction::ToggleStartup),
        MENU_ID_QUIT => Some(TrayAction::Quit),
        _ => None,
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
pub struct TrayOptions {
    pub port: u16,
    pub token: String,
    pub config_path: PathBuf,
    pub shutdown_tx: watch::Sender<bool>,
}

#[cfg_attr(not(windows), allow(dead_code))]
fn format_dashboard_url(port: u16, token: &str) -> String {
    format!("http://127.0.0.1:{port}/#token={token}")
}

#[cfg_attr(not(windows), allow(dead_code))]
fn format_admin_url(port: u16, token: &str) -> String {
    format!("http://127.0.0.1:{port}/admin#token={token}")
}

#[cfg_attr(not(windows), allow(dead_code))]
fn encode_tooltip_utf16(text: &str) -> [u16; 128] {
    let mut buf = [0u16; 128];
    for (dst, unit) in buf.iter_mut().take(127).zip(text.encode_utf16()) {
        *dst = unit;
    }
    buf
}

#[cfg(windows)]
mod platform {
    use super::{
        encode_tooltip_utf16, format_admin_url, format_dashboard_url, tray_action_from_menu_id,
        TrayAction, TrayOptions, MENU_ID_EDIT_CONFIG, MENU_ID_OPEN_ADMIN, MENU_ID_OPEN_BROWSER,
        MENU_ID_OPEN_PHONE, MENU_ID_QUIT, MENU_ID_TOGGLE_STARTUP,
    };
    use std::cell::RefCell;
    use std::ffi::c_void;
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{mpsc, Arc};
    use std::thread::{self, JoinHandle};

    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{
        GetLastError, ERROR_CLASS_ALREADY_EXISTS, HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM,
    };
    use windows::Win32::Graphics::Gdi::{
        CreateBitmap, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, BITMAPINFO,
        BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP,
    };
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Shell::{
        Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreateIconIndirect, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
        DestroyIcon, DestroyMenu, DestroyWindow, DispatchMessageW, GetCursorPos, GetMessageW,
        LoadIconW, PostMessageW, PostQuitMessage, RegisterClassW, SetForegroundWindow,
        TrackPopupMenu, TranslateMessage, HICON, ICONINFO, IDI_APPLICATION, MF_CHECKED,
        MF_SEPARATOR, MF_STRING, MF_UNCHECKED, MSG, TPM_BOTTOMALIGN, TPM_LEFTALIGN, TPM_NONOTIFY,
        TPM_RETURNCMD, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_CANCELMODE, WM_CLOSE,
        WM_CONTEXTMENU, WM_DESTROY, WM_LBUTTONDBLCLK, WM_NULL, WM_RBUTTONUP, WNDCLASSW,
    };

    const TRAY_ICON_UID: u32 = 1;
    const WM_TRAY_CALLBACK: u32 = WM_APP + 1;
    const TRAY_TOOLTIP: &str = "DeskPulse \u{2014} Hub de Mesa";

    struct TrayThreadState {
        opts: TrayOptions,
        closed: Arc<AtomicBool>,
        icon_removed: Arc<AtomicBool>,
    }

    thread_local! {
        static TRAY_STATE: RefCell<Option<TrayThreadState>> = const { RefCell::new(None) };
    }

    pub struct TrayHandle {
        hwnd: isize,
        closed: Arc<AtomicBool>,
        icon_removed: Arc<AtomicBool>,
        thread: Option<JoinHandle<()>>,
    }

    impl Drop for TrayHandle {
        fn drop(&mut self) {
            if self.hwnd != 0 && !self.closed.load(Ordering::Acquire) {
                let hwnd = HWND(self.hwnd as *mut c_void);
                unsafe {
                    remove_tray_icon_once(hwnd, &self.icon_removed);
                    let _ = PostMessageW(hwnd, WM_CANCELMODE, WPARAM(0), LPARAM(0));
                    let _ = PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
                }
            }
            self.hwnd = 0;
            if let Some(handle) = self.thread.take() {
                let _ = handle.join();
            }
        }
    }

    pub fn spawn_tray(opts: TrayOptions) -> Result<TrayHandle, String> {
        let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<isize, String>>(1);
        let closed = Arc::new(AtomicBool::new(false));
        let icon_removed = Arc::new(AtomicBool::new(false));
        let closed_thread = Arc::clone(&closed);
        let icon_removed_thread = Arc::clone(&icon_removed);

        let thread = thread::Builder::new()
            .name("deskpulse-tray".to_string())
            .spawn(move || {
                run_tray_thread(opts, closed_thread, icon_removed_thread, ready_tx);
            })
            .map_err(|e| format!("failed to spawn tray thread: {e}"))?;

        match ready_rx.recv() {
            Ok(Ok(hwnd)) => Ok(TrayHandle {
                hwnd,
                closed,
                icon_removed,
                thread: Some(thread),
            }),
            Ok(Err(err)) => {
                let _ = thread.join();
                Err(err)
            }
            Err(_) => {
                let _ = thread.join();
                Err("tray thread terminated before initialization".to_string())
            }
        }
    }

    fn run_tray_thread(
        opts: TrayOptions,
        closed: Arc<AtomicBool>,
        icon_removed: Arc<AtomicBool>,
        ready_tx: mpsc::SyncSender<Result<isize, String>>,
    ) {
        unsafe {
            let hmodule = match GetModuleHandleW(None) {
                Ok(m) => m,
                Err(e) => {
                    let _ = ready_tx.send(Err(format!("GetModuleHandleW failed: {e}")));
                    return;
                }
            };
            let hinstance: HINSTANCE = hmodule.into();
            let class_name = w!("DeskPulseTrayWindowClass");

            let wnd_class = WNDCLASSW {
                lpfnWndProc: Some(tray_wnd_proc),
                hInstance: hinstance,
                lpszClassName: class_name,
                ..Default::default()
            };

            let atom = RegisterClassW(&wnd_class);
            if atom == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS {
                let _ = ready_tx.send(Err(format!(
                    "RegisterClassW failed: {:?}",
                    GetLastError()
                )));
                return;
            }

            let hwnd = match CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class_name,
                w!("DeskPulse Tray"),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                HWND(std::ptr::null_mut()),
                None,
                hinstance,
                None,
            ) {
                Ok(h) => h,
                Err(e) => {
                    let _ = ready_tx.send(Err(format!("CreateWindowExW failed: {e}")));
                    return;
                }
            };

            let (icon, owns_icon) = create_pulse_icon().unwrap_or_else(|| {
                let fallback = LoadIconW(None, IDI_APPLICATION).unwrap_or_default();
                (fallback, false)
            });

            let nid = NOTIFYICONDATAW {
                cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                hWnd: hwnd,
                uID: TRAY_ICON_UID,
                uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
                uCallbackMessage: WM_TRAY_CALLBACK,
                hIcon: icon,
                szTip: encode_tooltip_utf16(TRAY_TOOLTIP),
                ..Default::default()
            };

            if !Shell_NotifyIconW(NIM_ADD, &nid).as_bool() {
                if owns_icon && !icon.is_invalid() {
                    let _ = DestroyIcon(icon);
                }
                let _ = DestroyWindow(hwnd);
                let _ = ready_tx.send(Err("Shell_NotifyIconW(NIM_ADD) failed".to_string()));
                return;
            }

            TRAY_STATE.with(|cell| {
                *cell.borrow_mut() = Some(TrayThreadState {
                    opts,
                    closed: Arc::clone(&closed),
                    icon_removed: Arc::clone(&icon_removed),
                });
            });

            let _ = ready_tx.send(Ok(hwnd.0 as isize));

            let mut msg = MSG::default();
            while GetMessageW(&mut msg, HWND(std::ptr::null_mut()), 0, 0).0 > 0 {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }

            closed.store(true, Ordering::Release);
            remove_tray_icon_once(hwnd, &icon_removed);
            if owns_icon && !icon.is_invalid() {
                let _ = DestroyIcon(icon);
            }
            TRAY_STATE.with(|cell| {
                *cell.borrow_mut() = None;
            });
        }
    }

    unsafe fn remove_tray_icon_once(hwnd: HWND, icon_removed: &AtomicBool) {
        if !icon_removed.swap(true, Ordering::AcqRel) {
            let nid = NOTIFYICONDATAW {
                cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                hWnd: hwnd,
                uID: TRAY_ICON_UID,
                ..Default::default()
            };
            let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
        }
    }

    unsafe fn create_pulse_icon() -> Option<(HICON, bool)> {
        const SIZE: i32 = 32;
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: SIZE,
                biHeight: -SIZE, // top-down DIB
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };

        let dc = CreateCompatibleDC(None);
        if dc.is_invalid() {
            return None;
        }

        let mut bits: *mut c_void = std::ptr::null_mut();
        let color_bmp: HBITMAP = match CreateDIBSection(dc, &bmi, DIB_RGB_COLORS, &mut bits, None, 0)
        {
            Ok(bmp) if !bmp.is_invalid() && !bits.is_null() => bmp,
            _ => {
                let _ = DeleteDC(dc);
                return None;
            }
        };

        let pixels = std::slice::from_raw_parts_mut(bits as *mut u32, (SIZE * SIZE) as usize);
        let center = 15.5_f32;
        for y in 0..SIZE {
            for x in 0..SIZE {
                let dx = x as f32 - center;
                let dy = y as f32 - center;
                let dist = (dx * dx + dy * dy).sqrt();
                let idx = (y * SIZE + x) as usize;
                if dist <= 15.0 {
                    // BGRA premultiplied-safe solid pixels
                    if dist >= 13.3 {
                        pixels[idx] = 0xFF06_B6D4; // Cyan ring (#06b6d4)
                    } else {
                        pixels[idx] = 0xFF0A_0F18; // OLED dark interior
                    }
                } else {
                    pixels[idx] = 0x0000_0000;
                }
            }
        }

        // Draw crisp DeskPulse heartbeat line across center
        let pulse_points: [(i32, i32); 7] = [
            (5, 16),
            (10, 16),
            (13, 9),
            (16, 23),
            (19, 13),
            (21, 16),
            (26, 16),
        ];
        for pair in pulse_points.windows(2) {
            draw_thick_segment(pixels, SIZE, pair[0], pair[1], 0xFF22_D3EE);
        }

        let mask_bits = [0u8; ((SIZE * SIZE) / 8) as usize];
        let mask_bmp = CreateBitmap(
            SIZE,
            SIZE,
            1,
            1,
            Some(mask_bits.as_ptr() as *const c_void),
        );
        if mask_bmp.is_invalid() {
            let _ = DeleteObject(color_bmp);
            let _ = DeleteDC(dc);
            return None;
        }

        let icon_info = ICONINFO {
            fIcon: true.into(),
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask_bmp,
            hbmColor: color_bmp,
        };

        let icon = CreateIconIndirect(&icon_info).ok();
        let _ = DeleteObject(mask_bmp);
        let _ = DeleteObject(color_bmp);
        let _ = DeleteDC(dc);

        icon.filter(|h| !h.is_invalid()).map(|h| (h, true))
    }

    fn draw_thick_segment(
        pixels: &mut [u32],
        size: i32,
        start: (i32, i32),
        end: (i32, i32),
        color: u32,
    ) {
        let steps = (end.0 - start.0).abs().max((end.1 - start.1).abs()).max(1);
        for i in 0..=steps {
            let t = i as f32 / steps as f32;
            let px = (start.0 as f32 + (end.0 - start.0) as f32 * t).round() as i32;
            let py = (start.1 as f32 + (end.1 - start.1) as f32 * t).round() as i32;
            for oy in -1i32..=1 {
                for ox in -1i32..=1 {
                    if ox.abs() + oy.abs() <= 1 {
                        let x = px + ox;
                        let y = py + oy;
                        if x >= 0 && x < size && y >= 0 && y < size {
                            pixels[(y * size + x) as usize] = color;
                        }
                    }
                }
            }
        }
    }

    fn open_url_in_browser(url: &str) {
        let mut cmd = Command::new("explorer.exe");
        cmd.arg(url);
        cmd.creation_flags(crate::monitors::CREATE_NO_WINDOW);
        let _ = cmd.spawn();
    }

    fn open_browser(port: u16, token: &str) {
        open_url_in_browser(&format_dashboard_url(port, token));
    }

    fn open_admin(port: u16, token: &str) {
        open_url_in_browser(&format_admin_url(port, token));
    }

    fn open_phone() {
        if let Ok(exe) = std::env::current_exe() {
            let mut cmd = Command::new(exe);
            cmd.arg("open");
            cmd.creation_flags(crate::monitors::CREATE_NO_WINDOW);
            let _ = cmd.spawn();
        }
    }

    fn open_config_editor(config_path: &std::path::Path) {
        let mut cmd = Command::new("notepad.exe");
        cmd.arg(config_path);
        let _ = cmd.spawn();
    }

    fn execute_action(hwnd: HWND, action: TrayAction) {
        let snapshot = TRAY_STATE.with(|cell| {
            cell.borrow().as_ref().map(|s| {
                (
                    s.opts.port,
                    s.opts.token.clone(),
                    s.opts.config_path.clone(),
                    s.opts.shutdown_tx.clone(),
                )
            })
        });

        let Some((port, token, config_path, shutdown_tx)) = snapshot else {
            return;
        };

        match action {
            TrayAction::OpenPhone => open_phone(),
            TrayAction::OpenAdmin => open_admin(port, &token),
            TrayAction::OpenBrowser => open_browser(port, &token),
            TrayAction::EditConfig => open_config_editor(&config_path),
            TrayAction::ToggleStartup => {
                if crate::config::startup_status() {
                    let _ = crate::config::uninstall_startup();
                } else {
                    let _ = crate::config::install_startup();
                }
            }
            TrayAction::Quit => {
                let _ = shutdown_tx.send(true);
                unsafe {
                    let _ = DestroyWindow(hwnd);
                }
            }
        }
    }

    unsafe fn show_context_menu(hwnd: HWND) {
        let Ok(hmenu) = CreatePopupMenu() else {
            return;
        };

        let startup_flags = MF_STRING
            | if crate::config::startup_status() {
                MF_CHECKED
            } else {
                MF_UNCHECKED
            };

        let _ = AppendMenuW(
            hmenu,
            MF_STRING,
            MENU_ID_OPEN_ADMIN as usize,
            w!("Painel Administrativo (/admin)"),
        );
        let _ = AppendMenuW(
            hmenu,
            MF_STRING,
            MENU_ID_OPEN_PHONE as usize,
            w!("Abrir no Celular (ADB)"),
        );
        let _ = AppendMenuW(
            hmenu,
            MF_STRING,
            MENU_ID_OPEN_BROWSER as usize,
            w!("Abrir no Navegador"),
        );
        let _ = AppendMenuW(
            hmenu,
            MF_STRING,
            MENU_ID_EDIT_CONFIG as usize,
            w!("Editar Config (config.json)"),
        );
        let _ = AppendMenuW(
            hmenu,
            startup_flags,
            MENU_ID_TOGGLE_STARTUP as usize,
            w!("Iniciar com o Windows"),
        );
        let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(hmenu, MF_STRING, MENU_ID_QUIT as usize, w!("Sair"));

        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let _ = SetForegroundWindow(hwnd);

        let selected = TrackPopupMenu(
            hmenu,
            TPM_LEFTALIGN | TPM_BOTTOMALIGN | TPM_RETURNCMD | TPM_NONOTIFY,
            pt.x,
            pt.y,
            0,
            hwnd,
            None,
        );
        let _ = PostMessageW(hwnd, WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(hmenu);

        if selected.0 > 0 {
            if let Some(action) = tray_action_from_menu_id(selected.0 as u32) {
                execute_action(hwnd, action);
            }
        }
    }

    unsafe extern "system" fn tray_wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_TRAY_CALLBACK => {
                let mouse_msg = (lparam.0 & 0xFFFF) as u32;
                match mouse_msg {
                    WM_LBUTTONDBLCLK => {
                        execute_action(hwnd, TrayAction::OpenBrowser);
                        LRESULT(0)
                    }
                    WM_RBUTTONUP | WM_CONTEXTMENU => {
                        show_context_menu(hwnd);
                        LRESULT(0)
                    }
                    _ => DefWindowProcW(hwnd, msg, wparam, lparam),
                }
            }
            WM_CLOSE => {
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            WM_DESTROY => {
                TRAY_STATE.with(|cell| {
                    if let Some(state) = cell.borrow().as_ref() {
                        state.closed.store(true, Ordering::Release);
                        remove_tray_icon_once(hwnd, &state.icon_removed);
                    }
                });
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use super::TrayOptions;

    pub struct TrayHandle;

    pub fn spawn_tray(_opts: TrayOptions) -> Result<TrayHandle, String> {
        Ok(TrayHandle)
    }
}

pub use platform::{spawn_tray, TrayHandle};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tray_menu_id_mapping_covers_all_actions() {
        assert_eq!(
            tray_action_from_menu_id(MENU_ID_OPEN_PHONE),
            Some(TrayAction::OpenPhone)
        );
        assert_eq!(
            tray_action_from_menu_id(MENU_ID_OPEN_ADMIN),
            Some(TrayAction::OpenAdmin)
        );
        assert_eq!(
            tray_action_from_menu_id(MENU_ID_OPEN_BROWSER),
            Some(TrayAction::OpenBrowser)
        );
        assert_eq!(
            tray_action_from_menu_id(MENU_ID_EDIT_CONFIG),
            Some(TrayAction::EditConfig)
        );
        assert_eq!(
            tray_action_from_menu_id(MENU_ID_TOGGLE_STARTUP),
            Some(TrayAction::ToggleStartup)
        );
        assert_eq!(
            tray_action_from_menu_id(MENU_ID_QUIT),
            Some(TrayAction::Quit)
        );
        assert_eq!(tray_action_from_menu_id(0), None);
        assert_eq!(tray_action_from_menu_id(9999), None);
    }

    #[test]
    fn test_format_dashboard_url_and_tooltip_encoding() {
        assert_eq!(
            format_dashboard_url(8765, "deadbeef"),
            "http://127.0.0.1:8765/#token=deadbeef"
        );
        assert_eq!(
            format_admin_url(8765, "deadbeef"),
            "http://127.0.0.1:8765/admin#token=deadbeef"
        );

        let tip = encode_tooltip_utf16("DeskPulse \u{2014} Hub de Mesa");
        assert_eq!(tip[127], 0);
        let decoded: String = tip
            .iter()
            .copied()
            .take_while(|&c| c != 0)
            .collect::<Vec<u16>>()
            .pipe_utf16_decode();
        assert_eq!(decoded, "DeskPulse \u{2014} Hub de Mesa");

        // Long tooltip is safely truncated with trailing NUL
        let long = "A".repeat(200);
        let truncated = encode_tooltip_utf16(&long);
        assert_eq!(truncated[126], b'A' as u16);
        assert_eq!(truncated[127], 0);
    }

    trait Utf16DecodeExt {
        fn pipe_utf16_decode(self) -> String;
    }

    impl Utf16DecodeExt for Vec<u16> {
        fn pipe_utf16_decode(self) -> String {
            String::from_utf16_lossy(&self)
        }
    }
}
