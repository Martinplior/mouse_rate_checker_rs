use std::{mem::MaybeUninit, time::Instant};

use windows::Win32::UI::{
    Input::{GetRawInputData, HRAWINPUT, RAWINPUT, RAWINPUTHEADER, RID_INPUT, RIM_TYPEMOUSE},
    WindowsAndMessaging::{MSG, WM_INPUT},
};

use crossbeam::channel::Sender as MpscSender;

pub fn create_msg_hook(
    msg_sender: MpscSender<Instant>,
) -> impl FnMut(*const std::ffi::c_void) -> bool {
    move |msg| {
        let msg = unsafe { &*(msg as *const MSG) };
        match msg.message {
            WM_INPUT => {
                handle_raw_input(msg, &msg_sender);
                true
            }
            _ => false,
        }
    }
}

#[inline(always)]
fn handle_raw_input(msg: &MSG, msg_sender: &MpscSender<Instant>) {
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
    let instant_now = Instant::now();
    msg_sender.send(instant_now).unwrap();
}
