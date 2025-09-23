use crate::{msgs::UpdateWorkspaces, window::settings::Settings};
use komorebi_client::{DefaultLayout, Layout, Ring, SocketMessage, Workspace};
use windows::Win32::UI::WindowsAndMessaging::WM_SETTINGCHANGE;
use winsafe::{prelude::*, *};

mod settings;

seq_ids! {
    ID_EXIT = 1001;
}
pub struct Window {
    pub hwnd: HWND,
    workspaces: Ring<Workspace>,
    settings: Settings,
}

const TEXT_PADDING: i32 = 20; // Padding around text in pixels

impl Window {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            hwnd: HWND::NULL,
            workspaces: loop {
                let Ok(new_workspaces) = crate::komo::read_workspaces() else {
                    log::error!("Failed to read workspaces, retrying...");
                    std::thread::sleep(std::time::Duration::from_secs(2));
                    continue;
                };
                break new_workspaces;
            },
            settings: Settings::new()?,
        })
    }

    pub fn register_class(&self, hinst: &HINSTANCE, class_name: &str) -> anyhow::Result<ATOM> {
        let mut wcx = WNDCLASSEX::default();
        wcx.lpfnWndProc = Some(Self::wnd_proc);
        wcx.hInstance = unsafe { hinst.raw_copy() };
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
                co::WS_EX::NOACTIVATE
                    | co::WS_EX::LAYERED
                    | co::WS_EX::TOOLWINDOW
                    | co::WS_EX::TOPMOST,
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

        if msg == co::WM::NCDESTROY {
            log::info!("HWND NCDESTROY: {:#?}", hwnd);
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
        const SETTINGCHANGED: co::WM = unsafe { co::WM::from_raw(WM_SETTINGCHANGE) };
        match p.msg_id {
            co::WM::CREATE => self.handle_create(),
            co::WM::PAINT => self.handle_paint(),
            co::WM::LBUTTONDOWN => {
                self.handle_lbuttondown(unsafe { msg::wm::RButtonDown::from_generic_wm(p) })
            }
            co::WM::RBUTTONDOWN => {
                self.handle_rbuttondown(unsafe { msg::wm::RButtonDown::from_generic_wm(p) })
            }
            co::WM::COMMAND => self.handle_command(unsafe { msg::wm::Command::from_generic_wm(p) }),
            UpdateWorkspaces::ID => self.handle_update_workspaces(UpdateWorkspaces::from_wndmsg(p)),
            SETTINGCHANGED => self.handle_setting_changed(),
            co::WM::DESTROY => {
                PostQuitMessage(0);
                Ok(0)
            }
            _ => Ok(unsafe { self.hwnd.DefWindowProc(p) }),
        }
    }

    fn handle_command(&mut self, mut p: msg::wm::Command) -> anyhow::Result<isize> {
        match p.event.ctrl_id() {
            ID_EXIT => {
                log::info!("Exiting application...");
                unsafe {
                    self.hwnd
                        .PostMessage(msg::WndMsg::new(co::WM::CLOSE, 0, 0))?;
                }
                Ok(0)
            }
            _ => Ok(unsafe { self.hwnd.DefWindowProc(p.as_generic_wm()) }),
        }
    }

    fn handle_rbuttondown(&mut self, p: msg::wm::RButtonDown) -> anyhow::Result<isize> {
        log::info!("Handling WM_RBUTTONDOWN message");
        log::info!("Cursor at: ({}, {})", p.coords.x, p.coords.y);
        let mut menu = HMENU::CreatePopupMenu()?;
        menu.append_item(&[winsafe::MenuItem::Entry {
            cmd_id: ID_EXIT,
            text: "Quit",
        }])?;

        menu.track_popup_menu_at_point(p.coords, &self.hwnd, &self.hwnd)?;
        log::debug!("Menu displayed");
        menu.DestroyMenu()?;
        log::debug!("Menu destroyed");
        Ok(0)
    }
    fn handle_lbuttondown(&mut self, p: msg::wm::RButtonDown) -> anyhow::Result<isize> {
        log::info!("Handling WM_LBUTTONDOWN message");
        let mut left = 0;
        let hdc = self.hwnd.GetDC()?;
        let rect = self.hwnd.GetClientRect()?;
        let focused_idx = self.workspaces.focused_idx();
        for (idx, workspace) in self.workspaces.elements().iter().enumerate() {
            let workspace_name = workspace.name.clone().unwrap_or((idx + 1).to_string());
            let sz = hdc.GetTextExtentPoint32(&workspace_name)?;

            let h_padding = if focused_idx == idx { 5 } else { 10 };
            let focused_rect = RECT {
                left: left + h_padding,
                right: left + sz.cx + TEXT_PADDING * 2 - h_padding,
                top: rect.bottom - 20,
                bottom: rect.bottom - 10,
            };

            if p.coords.x >= focused_rect.left && p.coords.x <= focused_rect.right {
                log::info!("Switching to workspace {}: {}", idx, workspace_name);
                komorebi_client::send_query(&SocketMessage::FocusWorkspaceNumber(idx))?;
                break;
            }

            left += sz.cx + TEXT_PADDING * 2;
        }
        Ok(0)
    }

    fn handle_setting_changed(&mut self) -> anyhow::Result<isize> {
        log::info!("Handling WM_SETTINGCHANGE message");
        self.settings = Settings::new()?;
        self.hwnd.SetLayeredWindowAttributes(
            self.settings.colors.get_color_key(),
            0,
            co::LWA::COLORKEY,
        )?;
        self.resize_to_fit()?;
        self.hwnd.InvalidateRect(None, true)?;
        Ok(0)
    }

    fn paint_and_get_width(&self, hdc: &HDC, paint: bool) -> anyhow::Result<i32> {
        let _old_font = hdc.SelectObject(&self.settings.font)?;

        let rect = if paint {
            self.hwnd.GetClientRect()?
        } else {
            RECT::default()
        };

        if paint {
            let _old_pen = hdc.SelectObject(&self.settings.transparent_pen)?;
            hdc.FillRect(rect, &self.settings.transparent_brush)?;
            hdc.SetTextColor(self.settings.colors.foreground)?;
            hdc.SetBkMode(co::BKMODE::TRANSPARENT)?;
        }

        const BORDER_RADIUS: SIZE = SIZE { cx: 10, cy: 10 };

        let mut left = 0;

        let focused_idx = self.workspaces.focused_idx();
        for (idx, workspace) in self.workspaces.elements().iter().enumerate() {
            let workspace_name = workspace.name.clone().unwrap_or((idx + 1).to_string());
            let sz = hdc.GetTextExtentPoint32(&workspace_name)?;

            if paint {
                let text_rect = RECT {
                    left,
                    right: left + sz.cx + TEXT_PADDING * 2,
                    top: 0,
                    bottom: rect.bottom - 10,
                };
                hdc.DrawText(
                    &workspace_name,
                    text_rect,
                    co::DT::CENTER | co::DT::VCENTER | co::DT::SINGLELINE,
                )?;

                let h_padding = if focused_idx == idx { 5 } else { 10 };

                let focused_rect = RECT {
                    left: left + h_padding,
                    right: left + sz.cx + TEXT_PADDING * 2 - h_padding,
                    top: rect.bottom - 20,
                    bottom: rect.bottom - 10,
                };

                let focused_brush = HBRUSH::CreateSolidBrush(if focused_idx == idx {
                    self.settings.colors.focused
                } else if workspace.is_empty() {
                    self.settings.colors.empty
                } else {
                    self.settings.colors.nonempty
                })?;
                let _old_brush = hdc.SelectObject(&*focused_brush);
                hdc.RoundRect(focused_rect, BORDER_RADIUS)?;
            }

            left += sz.cx + TEXT_PADDING * 2;
        }

        if let Some(cw) = self.workspaces.focused() {
            let mut current_state = String::new();

            if let Some(hwnd) = komorebi_client::WindowsApi::foreground_window().ok() {
                if let Some(window) = cw.maximized_window() {
                    if hwnd == window.hwnd {
                        current_state = "Maximized".to_string();
                    }
                }
                if let Some(container) = cw.monocle_container() {
                    if container.contains_window(hwnd) {
                        current_state = "Monocle".to_string();
                    }
                }
            }

            if current_state.is_empty() {
                if matches!(cw.layout, Layout::Default(DefaultLayout::Scrolling)) {
                    let focused_idx = cw.containers.focused_idx();
                    let total_containers = cw.containers().len();

                    if total_containers > 1 {
                        let draw_small_box = |text: &String,
                                              padding: i32,
                                              bg_color: COLORREF,
                                              lb: &mut i32,
                                              v_padding: i32|
                         -> anyhow::Result<()> {
                            const TEXT_WIDTH: i32 = 20;
                            if paint {
                                let text_rect = RECT {
                                    left: *lb,
                                    right: *lb + TEXT_WIDTH + padding * 2,
                                    top: rect.top + v_padding,
                                    bottom: rect.bottom - v_padding,
                                };

                                let focused_brush = HBRUSH::CreateSolidBrush(bg_color)?;
                                let _old_brush = hdc.SelectObject(&*focused_brush);
                                hdc.RoundRect(text_rect, BORDER_RADIUS)?;
                                if !text.is_empty() {
                                    hdc.DrawText(
                                        text,
                                        text_rect,
                                        co::DT::CENTER | co::DT::VCENTER | co::DT::SINGLELINE,
                                    )?;
                                }
                            }

                            *lb += TEXT_WIDTH + padding * 2;

                            Ok(())
                        };

                        left += TEXT_PADDING;

                        if total_containers >= 3 {
                            draw_small_box(
                                &(if focused_idx > 1 {
                                    "•".to_string()
                                } else {
                                    "".to_string()
                                }),
                                0,
                                self.settings.colors.get_color_key(),
                                &mut left,
                                20,
                            )?;
                        }
                        if total_containers > 2 || (total_containers == 2 && focused_idx == 1) {
                            draw_small_box(
                                &(if focused_idx > 0 {
                                    (focused_idx).to_string()
                                } else {
                                    "".to_string()
                                }),
                                12,
                                if focused_idx > 0 {
                                    self.settings.colors.empty
                                } else {
                                    self.settings.colors.get_color_key()
                                },
                                &mut left,
                                16,
                            )?;
                        }
                        draw_small_box(
                            &(focused_idx + 1).to_string(),
                            16,
                            self.settings.colors.nonempty,
                            &mut left,
                            14,
                        )?;
                        if total_containers >= 2 {
                            draw_small_box(
                                &(if focused_idx + 1 < total_containers {
                                    (focused_idx + 2).to_string()
                                } else {
                                    "".to_string()
                                }),
                                12,
                                if focused_idx + 1 < total_containers {
                                    self.settings.colors.empty
                                } else {
                                    self.settings.colors.get_color_key()
                                },
                                &mut left,
                                16,
                            )?;
                        }
                        if total_containers >= 3 {
                            draw_small_box(
                                &(if focused_idx + 2 < total_containers {
                                    "•".to_string()
                                } else {
                                    "".to_string()
                                }),
                                0,
                                self.settings.colors.get_color_key(),
                                &mut left,
                                20,
                            )?;
                        }
                    }
                }
            } else {
                let sz = hdc.GetTextExtentPoint32(&current_state)?;
                if paint {
                    let text_rect = RECT {
                        left: left,
                        right: left + sz.cx + TEXT_PADDING * 2,
                        top: rect.top + 12,
                        bottom: rect.bottom - 12,
                    };

                    let focused_brush =
                        HBRUSH::CreateSolidBrush(if current_state == "Maximized" {
                            self.settings.colors.nonempty
                        } else {
                            self.settings.colors.monocle
                        })?;
                    let _old_brush = hdc.SelectObject(&*focused_brush);
                    hdc.RoundRect(text_rect, BORDER_RADIUS)?;
                    hdc.DrawText(
                        &current_state,
                        text_rect,
                        co::DT::CENTER | co::DT::VCENTER | co::DT::SINGLELINE,
                    )?;
                }

                left += sz.cx + TEXT_PADDING * 2;
            }
        }

        Ok(left)
    }

    fn get_window_width(&self) -> anyhow::Result<i32> {
        let hdc = self.hwnd.GetDC()?;
        self.paint_and_get_width(&*hdc, false)
    }

    fn resize_to_fit(&self) -> anyhow::Result<bool> {
        let total_width = self.get_window_width()?;

        let rect = self.hwnd.GetClientRect()?;

        if rect.right - rect.left == total_width {
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

        Ok(true)
    }
    pub fn handle_update_workspaces(
        &mut self,
        workspaces: Ring<Workspace>,
    ) -> anyhow::Result<isize> {
        self.workspaces = workspaces;
        self.resize_to_fit()?;
        self.hwnd.InvalidateRect(None, true)?;
        Ok(0)
    }

    fn handle_create(&self) -> anyhow::Result<isize> {
        log::info!("Handling WM_CREATE message");
        Ok(0)
    }

    fn handle_paint(&self) -> anyhow::Result<isize> {
        log::info!("Handling WM_PAINT message...");
        let hdc = self.hwnd.BeginPaint()?;
        self.paint_and_get_width(&*hdc, true)?;
        log::info!("WM_PAINT handled.");
        Ok(0)
    }

    fn cleanup(&mut self) {
        self.hwnd = HWND::NULL;
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
            POINT { x: 15, y: 0 },
            SIZE {
                cx: self.get_window_width()?,
                cy: rect.bottom - rect.top,
            },
            &hinstance,
        )?;

        self.hwnd.SetParent(&taskbar)?;

        self.hwnd.SetLayeredWindowAttributes(
            self.settings.colors.get_color_key(),
            0,
            co::LWA::COLORKEY,
        )?;

        Ok(())
    }
}
