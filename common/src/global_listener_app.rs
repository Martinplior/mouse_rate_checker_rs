use std::{mem::MaybeUninit, os::windows::io::FromRawHandle, time::Instant};

use windows::Win32::{
    Devices::HumanInterfaceDevice::{HID_USAGE_GENERIC_MOUSE, HID_USAGE_PAGE_GENERIC},
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        Input::{
            GetRawInputData, RegisterRawInputDevices, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE,
            RAWINPUTHEADER, RIDEV_INPUTSINK, RID_INPUT, RIM_TYPEMOUSE,
        },
        WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, RegisterClassExW,
            HWND_MESSAGE, MSG, WM_INPUT, WNDCLASSEXW,
        },
    },
};

use crate::{interprocess_channel, main_app};

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct InstantWrap(Instant);

unsafe impl bytemuck::NoUninit for InstantWrap {}
unsafe impl bytemuck::AnyBitPattern for InstantWrap {}
unsafe impl bytemuck::Zeroable for InstantWrap {}

impl From<Instant> for InstantWrap {
    fn from(value: Instant) -> Self {
        Self(value)
    }
}

impl Into<Instant> for InstantWrap {
    fn into(self) -> Instant {
        self.0
    }
}

pub struct MainApp;

impl MainApp {
    pub const UNIQUE_IDENT: &str = "global_listener_app::MainApp::UNIQUE_IDENT";

    pub fn new() -> Self {
        Self
    }

    pub fn run(self) {
        let args: Box<[_]> = std::env::args().collect();
        let len = args.len();
        if len < 3 {
            panic!("违法参数！");
        }
        if args[1] != Self::UNIQUE_IDENT {
            panic!("违法参数！");
        }
        let raw_handle = usize::from_str_radix(&args[2], 10).unwrap();

        let owned_handle =
            unsafe { std::os::windows::io::OwnedHandle::from_raw_handle(raw_handle as _) };
        let msg_sender = interprocess_channel::Sender::<InstantWrap>::from(owned_handle);
        let cap = main_app::MainApp::BUF_CAP;
        let msg_sender = interprocess_channel::NonBlockSender::bounded(msg_sender, cap);

        let window_class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as _,
            lpfnWndProc: Some(Self::wnd_proc),
            hInstance: unsafe { GetModuleHandleW(None) }.unwrap().into(),
            lpszClassName: windows::core::w!("global_listener_window_class"),
            ..Default::default()
        };
        if unsafe { RegisterClassExW(&window_class) } == 0 {
            panic!("RegisterClassExW failed!");
        }

        let hwnd = unsafe {
            CreateWindowExW(
                Default::default(),
                window_class.lpszClassName,
                windows::core::w!("global_listener_msg_window"),
                Default::default(),
                0,
                0,
                0,
                0,
                HWND_MESSAGE,
                None,
                None,
                None,
            )
        }
        .unwrap();

        let raw_input_device = RAWINPUTDEVICE {
            usUsagePage: HID_USAGE_PAGE_GENERIC,
            usUsage: HID_USAGE_GENERIC_MOUSE,
            dwFlags: RIDEV_INPUTSINK,
            hwndTarget: hwnd,
        };
        unsafe {
            RegisterRawInputDevices(
                &[raw_input_device],
                std::mem::size_of::<RAWINPUTDEVICE>() as _,
            )
        }
        .unwrap();

        loop {
            let mut msg = MaybeUninit::uninit();
            if !unsafe { GetMessageW(msg.as_mut_ptr(), hwnd, 0, 0) }.as_bool() {
                break;
            }
            let instant_now = Instant::now();
            let msg = unsafe { msg.assume_init() };
            Self::handle_raw_input(&msg, &msg_sender, instant_now);
            unsafe { DispatchMessageW(&msg) };
        }
    }

    #[inline(always)]
    fn handle_raw_input(
        msg: &MSG,
        msg_sender: &interprocess_channel::NonBlockSender<InstantWrap>,
        instant_now: Instant,
    ) {
        if msg.message != WM_INPUT {
            return;
        }
        let l_param = msg.lParam.0 as usize;
        let raw_input = {
            let mut raw_input = MaybeUninit::<RAWINPUT>::uninit();
            let mut size = std::mem::size_of::<RAWINPUT>() as _;
            let header_size = std::mem::size_of::<RAWINPUTHEADER>() as _;
            let r = unsafe {
                GetRawInputData(
                    HRAWINPUT(l_param as _),
                    RID_INPUT,
                    Some(raw_input.as_mut_ptr() as _),
                    &mut size,
                    header_size,
                )
            };
            if r == 0 || r as i32 == -1 {
                panic!("GetRawInputData Failed!");
            }
            unsafe { raw_input.assume_init() }
        };
        if raw_input.header.dwType != RIM_TYPEMOUSE.0 {
            return;
        }
        msg_sender.send(instant_now.into()).unwrap();
    }

    unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    }
}
