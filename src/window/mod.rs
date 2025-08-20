use crate::{
    komo::{Workspace, WorkspaceState},
    msgs::UpdateWorkspaces,
    window::settings::{Settings},
    workspaces::Workspaces,
};
use windows::Win32::UI::WindowsAndMessaging::WM_SETTINGCHANGE;
use winsafe::{prelude::*, *};

mod settings;

pub struct Window {
    pub hwnd: HWND,
    workspaces: Workspaces,
    settings: Settings,
}

const TEXT_PADDING: i32 = 20; // Padding around text in pixels

impl Window {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            hwnd: HWND::NULL, // Initialize with NULL
            workspaces: Workspaces::new(),
            settings: Settings::new()?,
        })
    }

    pub fn register_class(&self, hinst: &HINSTANCE, class_name: &str) -> anyhow::Result<ATOM> {
        let mut wcx = WNDCLASSEX::default();
        wcx.lpfnWndProc = Some(Self::wnd_proc);
        wcx.hInstance = unsafe { hinst.raw_copy() };
        // wcx.hbrBackground = unsafe { self.settings.transparent_brush.raw_copy() };
        wcx.hCursor = HINSTANCE::NULL
            .LoadCursor(IdIdcStr::Idc(co::IDC::ARROW))?
            .leak();

        let mut wclass_name = if class_name.trim().is_empty() {
            WString::from_str(&format!(
                "WNDCLASS.{:#x}.{:#x}.{:#x}.{:#x}.{:#x}.{:#x}.{:#x}.{:#x}.{:#x}.{:#x}",
                wcx.style,
                wcx.lpfnWndProc.map_or(0, |p| p as usize),
                wcx.cbClsExtra,
                wcx.cbWndExtra,
                wcx.hInstance,
                wcx.hIcon,
                wcx.hCursor,
                wcx.hbrBackground,
                wcx.lpszMenuName(),
                wcx.hIconSm,
            ))
        } else {
            WString::from_str(class_name)
        };
        wcx.set_lpszClassName(Some(&mut wclass_name));

        SetLastError(co::ERROR::SUCCESS);
        match unsafe { RegisterClassEx(&wcx) } {
            Ok(atom) => Ok(atom),
            Err(err) => match err {
                co::ERROR::CLASS_ALREADY_EXISTS => {
                    // https://devblogs.microsoft.com/oldnewthing/20150429-00/?p=44984
                    // https://devblogs.microsoft.com/oldnewthing/20041011-00/?p=37603
                    // Retrieve ATOM of existing window class.
                    let hinst = unsafe { wcx.hInstance.raw_copy() };
                    let (atom, _) = hinst.GetClassInfoEx(&wcx.lpszClassName().unwrap())?;
                    Ok(atom)
                }
                err => panic!("ERROR: Window::register_class: {}", err.to_string()),
            },
        }
    }
    pub fn create_window(
        &mut self,
        class_name: ATOM,
        pos: POINT,
        size: SIZE,
        hinst: &HINSTANCE,
    ) -> anyhow::Result<()> {
        if self.hwnd != HWND::NULL {
            panic!("Cannot create window twice.");
        }

        unsafe {
            // The hwnd member is saved in WM_NCCREATE message
            HWND::CreateWindowEx(
                co::WS_EX::NOACTIVATE | co::WS_EX::LAYERED,
                AtomStr::Atom(class_name),
                None,
                co::WS::VISIBLE | co::WS::CLIPSIBLINGS | co::WS::POPUP,
                pos,
                size,
                None,
                IdMenu::None,
                hinst,
                Some(self as *const _ as _), // pass pointer to object itself
            )?
        };

        Ok(())
    }

    extern "system" fn wnd_proc(hwnd: HWND, msg: co::WM, wparam: usize, lparam: isize) -> isize {
        let wm_any = msg::WndMsg::new(msg, wparam, lparam);
        let ptr_self = match msg {
            co::WM::NCCREATE => {
                let msg = unsafe { msg::wm::NcCreate::from_generic_wm(wm_any) };
                let ptr_self = msg.createstruct.lpCreateParams as *mut Self;
                unsafe {
                    hwnd.SetWindowLongPtr(co::GWLP::USERDATA, ptr_self as _); // store
                }
                log::info!("HWND NCCREATE: {:#?}", hwnd);
                let ref_self = unsafe { &mut *ptr_self };
                ref_self.hwnd = unsafe { hwnd.raw_copy() };
                return unsafe { hwnd.DefWindowProc(wm_any) }; // continue processing
            }
            _ => hwnd.GetWindowLongPtr(co::GWLP::USERDATA) as *mut Self, // retrieve
        };

        if ptr_self.is_null() {
            log::error!("Received message for uninitialized window: {:#?}", wm_any);
            return unsafe { hwnd.DefWindowProc(wm_any) };
        }

        let ref_self = unsafe { &mut *ptr_self };
        // log::debug!("Dereferenced pointer to self");

        if msg == co::WM::NCDESTROY {
            unsafe {
                ref_self.hwnd.SetWindowLongPtr(co::GWLP::USERDATA, 0); // clear passed pointer
            }
            ref_self.cleanup();
            return 0;
        }

        ref_self.handle_message(wm_any).unwrap_or_else(|err| {
            log::error!("Application error: {err}");
            0
        })
    }

    fn handle_message(&mut self, p: msg::WndMsg) -> anyhow::Result<isize> {
        // log::debug!("Received message: {:#?}", p);
        const SETTINGCHANGED: co::WM = unsafe { co::WM::from_raw(WM_SETTINGCHANGE)};
        match p.msg_id {
            co::WM::CREATE => self.handle_create(),
            co::WM::PAINT => self.handle_paint(),
            UpdateWorkspaces::ID => self.handle_update_workspaces(UpdateWorkspaces::from_wndmsg(p)),
            SETTINGCHANGED => self.handle_setting_changed(),
            co::WM::DESTROY => {
                PostQuitMessage(0);
                Ok(0)
            },
            _ => Ok(unsafe { self.hwnd.DefWindowProc(p) }),
        }
    }

    fn handle_setting_changed(&mut self) -> anyhow::Result<isize> {
        log::debug!("Handling WM_SETTINGCHANGE message");
        // Here you can handle system settings changes, such as theme changes.
        // For example, you might want to update colors or fonts based on the new settings.
        self.settings = Settings::new()?;
        self.hwnd
            .SetLayeredWindowAttributes(self.settings.colors.get_color_key(), 0, co::LWA::COLORKEY)?;
        self.hwnd.InvalidateRect(None, false)?;
        Ok(0)
    }

    fn get_window_width(&self) -> anyhow::Result<i32> {
        let hdc = self.hwnd.GetDC()?;
        let _old_font = hdc.SelectObject(&self.settings.font)?;
        let width = self.workspaces.data.iter().fold(0, |acc, workspace| {
            let sz = hdc
                .GetTextExtentPoint32(&workspace.data.name)
                .unwrap_or_default();
            acc + sz.cx + TEXT_PADDING * 2 // add padding for each workspace
        });

        Ok(width)
    }

    fn resize_to_fit(&self) -> anyhow::Result<bool> {
        let total_width = self.get_window_width()?;

        let rect = self.hwnd.GetClientRect()?;

        if rect.right - rect.left == total_width {
            log::debug!("No resize needed, current width matches total width");
            return Ok(false);
        }

        self.hwnd.SetWindowPos(
            winsafe::HwndPlace::Place(co::HWND_PLACE::default()),
            POINT::default(),
            SIZE {
                cx: total_width,
                cy: rect.bottom - rect.top,
            },
            co::SWP::NOACTIVATE | co::SWP::NOZORDER | co::SWP::NOMOVE | co::SWP::NOREDRAW,
        )?;

        log::debug!("Finish resizing window pos");
        Ok(true)
    }
    pub fn handle_update_workspaces(
        &mut self,
        workspaces: Vec<Workspace>,
    ) -> anyhow::Result<isize> {
        log::debug!("Updating workspaces: {:?}", workspaces);
        if self.workspaces.try_update(workspaces) {
            if self.workspaces.name_changed() {
                self.resize_to_fit()?;
            }
            self.hwnd.InvalidateRect(None, false)?;
        }
        // Here you can implement the logic to update the window based on the new workspaces.
        // For example, you might want to redraw the window or update some internal state.
        Ok(0)
    }

    fn handle_create(&self) -> anyhow::Result<isize> {
        log::debug!("Handling WM_CREATE message");
        // Here you can perform any initialization needed when the window is created.
        // For example, setting up controls or initializing resources.
        Ok(0)
    }

    fn handle_paint(&self) -> anyhow::Result<isize> {
        log::debug!("WM_PAINT event received");

        let hdc = self.hwnd.BeginPaint()?;

        let rect = self.hwnd.GetClientRect()?;

        let _old_pen = hdc.SelectObject(&self.settings.transparent_pen)?;

        hdc.SetTextColor(self.settings.colors.foreground)?;
        hdc.SetBkMode(co::BKMODE::TRANSPARENT)?;
        let _old_font = hdc.SelectObject(&self.settings.font)?;

        let name_changed = self.workspaces.name_changed();
        let mut left = 0;
        for workspace in &self.workspaces.data {
            let sz = hdc.GetTextExtentPoint32(&workspace.data.name)?;
            if name_changed {
                let text_rect = RECT {
                    left,
                    right: left + sz.cx + TEXT_PADDING * 2,
                    top: 0,
                    bottom: rect.bottom - 10,
                };

                hdc.FillRect(text_rect, &self.settings.transparent_brush)?;

                hdc.DrawText(
                    &workspace.data.name,
                    text_rect,
                    co::DT::CENTER | co::DT::VCENTER | co::DT::SINGLELINE,
                )?;
            }

            let clear_rect = RECT {
                left,
                right: left + sz.cx + TEXT_PADDING * 2,
                top: rect.bottom - 20,
                bottom: rect.bottom,
            };

            let focused_rect = RECT {
                left: left + 5,
                right: left + sz.cx + TEXT_PADDING * 2 - 5,
                top: rect.bottom - 20,
                bottom: rect.bottom - 10,
            };

            let border_radius = SIZE { cx: 10, cy: 10 };

            if name_changed || workspace.state_changed {
                hdc.FillRect(clear_rect, &self.settings.transparent_brush)?;

                match workspace.data.state {
                    WorkspaceState::Focused => {
                        let focused_brush = HBRUSH::CreateSolidBrush(self.settings.colors.focused)?;
                        let _old_brush = hdc.SelectObject(&*focused_brush);
                        hdc.RoundRect(focused_rect, border_radius)?;
                    }
                    WorkspaceState::NonEmpty => {
                        let focused_rect = RECT {
                            left: left + 10,
                            right: left + sz.cx + TEXT_PADDING * 2 - 10,
                            top: rect.bottom - 20,
                            bottom: rect.bottom - 10,
                        };
                        let nonempty_brush = HBRUSH::CreateSolidBrush(self.settings.colors.nonempty)?;
                        let _old_brush = hdc.SelectObject(&*nonempty_brush);
                        hdc.RoundRect(focused_rect, border_radius)?;
                    }
                    WorkspaceState::Empty => {
                        // No special drawing for empty workspaces
                    }
                }
            }

            left += sz.cx + TEXT_PADDING * 2; // move left for next workspace
        }

        log::debug!("Drawn workspaces");
        log::debug!("self workspaces lock() finished");
        log::debug!("self settings lock() finished");
        log::info!("WM_PAINT event processed");
        Ok(0)
    }

    fn cleanup(&mut self) {
        self.hwnd = HWND::NULL; // Clear the HWND to prevent dangling references
    }

    pub fn run_loop(&self) -> anyhow::Result<()> {
        let mut msg = MSG::default();
        while GetMessage(&mut msg, None, 0, 0)? {
            TranslateMessage(&msg);
            unsafe {
                DispatchMessage(&msg);
            }
        }
        Ok(())
    }

    pub fn prepare(&mut self) -> anyhow::Result<()> {
        // Ensure the process is DPI aware for high DPI displays
        if IsWindowsVistaOrGreater()? {
            SetProcessDPIAware()?;
        }

        let hinstance = HINSTANCE::GetModuleHandle(None)?;

        let atom = self.register_class(&hinstance, "komoswitch")?;

        let taskbar_atom = AtomStr::from_str("Shell_TrayWnd");
        let taskbar = HWND::FindWindow(Some(taskbar_atom), None)?
            .ok_or(anyhow::anyhow!("Taskbar not found"))?;

        let rect = taskbar.GetClientRect()?;

        self.create_window(
            atom,
            POINT { 
                x: 15, 
                y: 0
            },
            SIZE {
                cx: self.get_window_width()?,
                cy: rect.bottom - rect.top, // Set initial size
            },
            &hinstance,
        )?;

        self.hwnd.SetParent(&taskbar)?;

        self.hwnd
            .SetLayeredWindowAttributes(self.settings.colors.get_color_key(), 0, co::LWA::COLORKEY)?;

        Ok(())
    }
}
