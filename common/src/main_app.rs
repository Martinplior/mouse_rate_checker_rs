use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

use eframe::{
    egui_wgpu::WgpuConfiguration,
    wgpu::{Backends, PowerPreference, PresentMode},
};
use egui::{ViewportBuilder, Widget};
use windows::Win32::UI::WindowsAndMessaging::{MSG, WM_INPUT};

use crossbeam::channel::Receiver as MpscReceiver;
use crossbeam::channel::Sender as MpscSender;

use crate::{global_listener::GlobalListener, win_utils};

pub struct MainApp;

impl MainApp {
    /// large enough to avoid jam
    pub const CHANNEL_CAP: usize = u16::MAX as usize + 1;

    const EDGE: f32 = 600.0;

    const SIDE_WIDTH: f32 = 200.0;

    pub fn new() -> Self {
        Self
    }

    pub fn run(self) {
        let edge = Self::EDGE;
        let native_options = eframe::NativeOptions {
            viewport: ViewportBuilder::default()
                .with_inner_size([edge + Self::SIDE_WIDTH, edge])
                .with_resizable(false)
                .with_maximize_button(false)
                .with_minimize_button(false),
            renderer: eframe::Renderer::Wgpu,
            wgpu_options: WgpuConfiguration {
                supported_backends: Backends::VULKAN,
                present_mode: PresentMode::AutoVsync,
                power_preference: PowerPreference::HighPerformance,
                ..Default::default()
            },
            ..Default::default()
        };

        eframe::run_native(
            "Mouse Rate Checker",
            native_options,
            Box::new(|cc| Ok(Box::new(App::new(cc)))),
        )
        .unwrap();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GotoBottomState {
    None,
    Discard,
    Scroll,
}

struct App {
    _global_listener: GlobalListener,
    msg_receiver: MpscReceiver<Instant>,
    msg_buf: Vec<Instant>,
    instant_queue: VecDeque<Instant>,
    last_reported_instant: Instant,

    scroll_offset: f32,
    need_goto_bottom: GotoBottomState,

    show_state: bool,
    text_label_buf: Vec<f32>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_theme(egui::Theme::Dark);
        let channel_cap = u16::MAX as usize + 1;
        let (msg_sender, msg_receiver) = crossbeam::channel::bounded(channel_cap);
        let global_listener = GlobalListener::new(Self::create_msg_hook(msg_sender), |&hwnd| {
            use win_utils::raw_input_device;
            raw_input_device::register(
                raw_input_device::DeviceType::Keyboard,
                raw_input_device::OptionType::Remove,
            );
            raw_input_device::register(
                raw_input_device::DeviceType::Mouse,
                raw_input_device::OptionType::Flags(hwnd, Default::default()),
            );
        });
        let capacity = 1024 * 64;
        let text_label_buf_capacity = 1024 * 1024;
        Self {
            _global_listener: global_listener,
            msg_receiver,
            msg_buf: Vec::with_capacity(capacity),
            instant_queue: VecDeque::with_capacity(capacity),
            last_reported_instant: Instant::now(),
            scroll_offset: 0.0,
            need_goto_bottom: GotoBottomState::None,
            show_state: true,
            text_label_buf: Vec::with_capacity(text_label_buf_capacity),
        }
    }

    fn create_msg_hook(msg_sender: MpscSender<Instant>) -> impl FnMut(&MSG) -> bool {
        move |msg| {
            if msg.message == WM_INPUT {
                Self::handle_raw_input(msg, &msg_sender);
                return true;
            }
            false
        }
    }

    fn handle_raw_input(msg: &MSG, msg_sender: &MpscSender<Instant>) {
        let raw_input = win_utils::RawInput::from_msg(msg);
        if !matches!(raw_input, win_utils::RawInput::Mouse(_)) {
            unreachable!("unexpected raw input");
        }
        msg_sender.send(Instant::now()).unwrap();
    }
}

impl App {
    fn show_text_labels(&mut self, ui: &mut egui::Ui) {
        ui.allocate_ui([100.0, 400.0].into(), |ui| {
            ui.set_width(100.0);
            ui.set_height(400.0);
            let mut scroll_area = egui::ScrollArea::vertical()
                .stick_to_bottom(true)
                .auto_shrink(false);
            match self.need_goto_bottom {
                GotoBottomState::Discard => {
                    ui.ctx().request_discard("need scroll offset");
                    self.need_goto_bottom = GotoBottomState::Scroll;
                }
                GotoBottomState::Scroll => {
                    scroll_area = scroll_area.vertical_scroll_offset(self.scroll_offset);
                    self.need_goto_bottom = GotoBottomState::None
                }
                _ => (),
            }
            let text_height = ui.text_style_height(&egui::TextStyle::Body);
            let r =
                scroll_area.show_rows(ui, text_height, self.text_label_buf.len(), |ui, range| {
                    ui.vertical(|ui| {
                        self.text_label_buf
                            .get(range)
                            .unwrap()
                            .iter()
                            .for_each(|&value| {
                                let text = if value >= 1000.0 {
                                    ">= 1s".to_string()
                                } else {
                                    format!("{:.3}ms", value)
                                };
                                ui.label(text);
                            });
                    });
                });
            self.scroll_offset = r.content_size.y - r.inner_rect.height();
        });
    }

    fn instant_queue_remove_ddl(&mut self, instant_now: Instant) {
        let dead_line = instant_now - Duration::from_secs(1);
        while let Some(&instant) = self.instant_queue.front() {
            if instant < dead_line {
                self.instant_queue.pop_front();
            } else {
                break;
            }
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let instant_now = Instant::now();

        self.msg_buf.extend(self.msg_receiver.try_iter());

        egui::SidePanel::right("right panel")
            .exact_width(MainApp::SIDE_WIDTH)
            .resizable(false)
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    egui::Checkbox::new(
                        &mut self.show_state,
                        egui::RichText::new("show").size(20.0),
                    )
                    .ui(ui);
                    egui::Frame::default()
                        .stroke(ui.visuals().noninteractive().bg_stroke)
                        .inner_margin(egui::Margin::same(5.0))
                        .show(ui, |ui| {
                            self.show_text_labels(ui);
                        });
                    ui.add_space(10.0);
                    ui.allocate_ui([160.0, 30.0].into(), |ui| {
                        ui.horizontal_centered(|ui| {
                            egui::Label::new(egui::RichText::new("average: ").size(20.0))
                                .selectable(false)
                                .ui(ui);
                            egui::Frame::default()
                                .stroke(ui.visuals().noninteractive().bg_stroke)
                                .inner_margin(egui::Margin::same(5.0))
                                .show(ui, |ui| {
                                    ui.set_width(60.0);
                                    ui.label(format!("{} hz", self.instant_queue.len()));
                                });
                        });
                    });
                });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.painter().text(
                ui.clip_rect().center(),
                egui::Align2::CENTER_CENTER,
                "Move your mouse!",
                egui::FontId::proportional(20.0),
                ui.visuals().text_color(),
            );

            if !ui.input(|i| i.focused) {
                return;
            }
            let response = ui.allocate_response(
                ui.available_size(),
                egui::Sense::hover() | egui::Sense::drag(),
            );
            if !(response.contains_pointer() && !self.msg_buf.is_empty()) {
                return;
            }

            if self.show_state {
                self.msg_buf.drain(..).for_each(|instant| {
                    self.instant_queue.push_back(instant);
                    let duration = instant - self.last_reported_instant;
                    self.text_label_buf.push(duration.as_secs_f32() * 1000.0);
                    self.last_reported_instant = instant;
                });
                self.instant_queue_remove_ddl(instant_now);
                if self.need_goto_bottom == GotoBottomState::None {
                    self.need_goto_bottom = GotoBottomState::Discard;
                }
            } else {
                self.instant_queue.extend(self.msg_buf.drain(..));
                self.instant_queue_remove_ddl(instant_now);
            }
        });
    }
}
