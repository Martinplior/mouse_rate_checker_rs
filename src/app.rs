use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

use eframe::{
    egui_wgpu::WgpuConfiguration,
    wgpu::{Backends, PowerPreference, PresentMode},
};
use egui::{ViewportBuilder, Widget};
use winit::platform::windows::EventLoopBuilderExtWindows;

use crate::msg_hook::create_msg_hook;

use crossbeam::channel::Receiver as MpscReceiver;

pub struct MainApp;

impl MainApp {
    const EDGE: f32 = 600.0;

    const SIDE_WIDTH: f32 = 200.0;

    pub fn new() -> Self {
        Self
    }

    pub fn run(self) {
        // large enough to avoid jam
        const CAP: usize = u16::MAX as usize + 1;
        let (msg_sender, msg_receiver) = crossbeam::channel::bounded(CAP);
        let edge = Self::EDGE;
        // let icon_data = {
        //     let img = image::load_from_memory(include_bytes!("../../icons/kps_icon.png")).unwrap();
        //     let width = img.width();
        //     let height = img.height();
        //     let data = img.into_bytes();
        //     egui::IconData {
        //         rgba: data,
        //         width,
        //         height,
        //     }
        // };
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
            event_loop_builder: Some(Box::new(|event_loop_builder| {
                event_loop_builder.with_msg_hook(create_msg_hook(msg_sender));
            })),
            ..Default::default()
        };
        eframe::run_native(
            "Mouse Rate Checker",
            native_options,
            Box::new(|cc| Ok(Box::new(App::new(cc, msg_receiver)))),
        )
        .unwrap();
    }
}

struct App {
    msg_receiver: MpscReceiver<Instant>,
    instant_queue: VecDeque<Instant>,

    last_pointer_pos: Option<egui::Pos2>,
    scroll_offset: f32,
    need_goto_bottom: bool,

    show_state: bool,
    text_label_buf: Vec<u32>,
    avg10_buf: VecDeque<u32>,
    avg10_sum: u32,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, msg_receiver: MpscReceiver<Instant>) -> Self {
        cc.egui_ctx.set_theme(egui::Theme::Dark);
        let capacity = 1024 * 64;
        let text_label_buf_capacity = 1024 * 1024;
        Self {
            msg_receiver,
            instant_queue: VecDeque::with_capacity(capacity),
            last_pointer_pos: None,
            scroll_offset: 0.0,
            need_goto_bottom: false,
            show_state: true,
            text_label_buf: Vec::with_capacity(text_label_buf_capacity),
            avg10_buf: (0..10).map(|_| 0).collect(),
            avg10_sum: 0,
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
            let need_goto_bottom = std::mem::take(&mut self.need_goto_bottom);
            if need_goto_bottom {
                scroll_area = scroll_area.vertical_scroll_offset(self.scroll_offset);
            }
            let text_height = ui.text_style_height(&egui::TextStyle::Body);
            let r =
                scroll_area.show_rows(ui, text_height, self.text_label_buf.len(), |ui, range| {
                    ui.vertical(|ui| {
                        self.text_label_buf
                            .get(range)
                            .unwrap()
                            .iter()
                            .for_each(|hz| {
                                ui.label(format!("{} hz", hz));
                            });
                    });
                });
            self.scroll_offset = r.content_size.y - r.inner_rect.height() + text_height;
        });
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let instant_now = std::time::Instant::now();
        self.instant_queue.extend(self.msg_receiver.try_iter());
        let dead_line = instant_now - Duration::from_secs(1);
        let count = self
            .instant_queue
            .iter()
            .take_while(|&instant| *instant < dead_line)
            .count();
        self.instant_queue.drain(..count);

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
                                    egui::Label::new(format!("{} hz", self.avg10_sum / 10)).ui(ui);
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

            let new_hz = self.instant_queue.len() as u32;
            let old_hz = self.avg10_buf.pop_front().unwrap();
            self.avg10_buf.push_back(new_hz);
            self.avg10_sum = self.avg10_sum.wrapping_add(new_hz).wrapping_sub(old_hz);

            if !self.show_state {
                return;
            }

            self.text_label_buf.push(new_hz);
            self.need_goto_bottom = true;

            ctx.request_repaint();
        });
    }
}
