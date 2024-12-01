use std::{
    collections::VecDeque,
    os::windows::io::IntoRawHandle,
    time::{Duration, Instant},
};

use eframe::{
    egui_wgpu::WgpuConfiguration,
    wgpu::{Backends, PowerPreference, PresentMode},
};
use egui::{ViewportBuilder, Widget};

use crate::{
    global_listener_app::{self, InstantWrap},
    interprocess_channel,
};

use interprocess_channel::NonBlockReceiver as MpscReceiver;

pub struct MainApp;

impl MainApp {
    /// large enough to avoid jam
    pub const BUF_CAP: usize = u16::MAX as usize + 1;

    const EDGE: f32 = 600.0;

    const SIDE_WIDTH: f32 = 200.0;

    pub fn new() -> Self {
        Self
    }

    pub fn run(self) {
        let cap = Self::BUF_CAP;
        let (msg_sender, msg_receiver) = interprocess_channel::bounded(cap).unwrap();
        let msg_receiver = MpscReceiver::bounded(msg_receiver, cap);

        let msg_sender_handle: std::os::windows::io::OwnedHandle = msg_sender.into();
        let msg_sender_raw_handle = msg_sender_handle.into_raw_handle() as usize;

        let path = crate::get_current_dir().join("global_listener_app");

        let mut global_listener = std::process::Command::new(path)
            .arg(global_listener_app::MainApp::UNIQUE_IDENT)
            .arg(msg_sender_raw_handle.to_string())
            .spawn()
            .unwrap();

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
            Box::new(|cc| Ok(Box::new(App::new(cc, msg_receiver)))),
        )
        .unwrap();

        let _ = global_listener.kill();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GotoBottomState {
    None,
    Discard,
    Scroll,
}

struct App {
    msg_receiver: MpscReceiver<InstantWrap>,
    msg_buf: Vec<Instant>,
    instant_queue: VecDeque<Instant>,
    last_reported_instant: Instant,

    last_pointer_pos: Option<egui::Pos2>,
    scroll_offset: f32,
    need_goto_bottom: GotoBottomState,

    show_state: bool,
    text_label_buf: Vec<f32>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, msg_receiver: MpscReceiver<InstantWrap>) -> Self {
        cc.egui_ctx.set_theme(egui::Theme::Dark);
        let capacity = 1024 * 64;
        let text_label_buf_capacity = 1024 * 1024;
        Self {
            msg_receiver,
            msg_buf: Vec::with_capacity(capacity),
            instant_queue: VecDeque::with_capacity(capacity),
            last_reported_instant: Instant::now(),
            last_pointer_pos: None,
            scroll_offset: 0.0,
            need_goto_bottom: GotoBottomState::None,
            show_state: true,
            text_label_buf: Vec::with_capacity(text_label_buf_capacity),
        }
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
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let instant_now = Instant::now();

        self.msg_buf.clear();
        self.msg_buf
            .extend(self.msg_receiver.try_iter().map(Into::<Instant>::into));

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
            let current_pos = ui.input(|i| i.pointer.latest_pos());
            if !(response.contains_pointer() && current_pos != self.last_pointer_pos) {
                return;
            }
            self.last_pointer_pos = current_pos;

            self.instant_queue.extend(self.msg_buf.iter());
            let dead_line = instant_now - Duration::from_secs(1);
            let count = self
                .instant_queue
                .iter()
                .take_while(|&instant| *instant < dead_line)
                .count();
            self.instant_queue.drain(..count);

            if !self.show_state {
                return;
            }

            self.msg_buf.iter().for_each(|&instant| {
                let duration = instant - self.last_reported_instant;
                self.text_label_buf.push(duration.as_secs_f32() * 1000.0);
                self.last_reported_instant = instant;
            });

            if self.need_goto_bottom == GotoBottomState::None {
                self.need_goto_bottom = GotoBottomState::Discard;
            }
        });
    }
}
