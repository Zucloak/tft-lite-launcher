use egui::{
    Align2, Color32, Context, FontDefinitions, FontFamily, FontId, Pos2, Rect, Response, Rounding,
    Sense, Stroke, Style, TextStyle, Ui, Vec2, Visuals,
};

use crate::icons::{icon_image, IconKind, IconStyle};

/// Minimalist macOS-inspired obsidian design tokens for TFT-Lite-Launcher.
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub canvas: Color32,
    pub sidebar_top: Color32,
    pub sidebar_bottom: Color32,
    pub glass_surface: Color32,
    pub glass_hover: Color32,
    pub popover_bg: Color32,
    pub hairline: Color32,
    pub hairline_strong: Color32,
    pub gold: Color32,
    pub gold_hover: Color32,
    pub gold_text: Color32,
    pub gold_tint: Color32,
    pub gold_fill: Color32,
    pub teal: Color32,
    pub danger: Color32,
    pub text_primary: Color32,
    pub text_secondary: Color32,
    pub text_muted: Color32,
    pub rounding_sm: f32,
    pub rounding_md: f32,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            canvas: Color32::from_rgb(0x0A, 0x0D, 0x12),
            sidebar_top: Color32::from_rgb(0x0D, 0x11, 0x18),
            sidebar_bottom: Color32::from_rgb(0x08, 0x0B, 0x10),
            glass_surface: Color32::from_rgba_unmultiplied(255, 255, 255, 10),
            glass_hover: Color32::from_rgba_unmultiplied(255, 255, 255, 18),
            popover_bg: Color32::from_rgba_unmultiplied(14, 18, 26, 238),
            hairline: Color32::from_rgba_unmultiplied(255, 255, 255, 15),
            hairline_strong: Color32::from_rgba_unmultiplied(255, 255, 255, 28),
            gold: Color32::from_rgb(0xC8, 0xAA, 0x6E),
            gold_hover: Color32::from_rgb(0xD6, 0xBC, 0x85),
            gold_text: Color32::from_rgb(0x0A, 0x0D, 0x12),
            gold_tint: Color32::from_rgba_unmultiplied(0xC8, 0xAA, 0x6E, 20),
            gold_fill: Color32::from_rgba_unmultiplied(0xC8, 0xAA, 0x6E, 72),
            teal: Color32::from_rgb(0x0A, 0xC8, 0xB9),
            danger: Color32::from_rgb(0xE0, 0x52, 0x52),
            text_primary: Color32::from_rgb(0xEC, 0xF1, 0xF7),
            text_secondary: Color32::from_rgb(0x8E, 0x9B, 0xAE),
            text_muted: Color32::from_rgb(0x5A, 0x65, 0x77),
            rounding_sm: 6.0,
            rounding_md: 8.0,
        }
    }
}

impl Theme {
    /// Applies font definitions and overrides egui's default dark visuals
    /// with flat obsidian surfaces and translucent popovers.
    pub fn apply(&self, ctx: &Context) {
        self.install_fonts(ctx);

        let mut visuals = Visuals::dark();
        visuals.override_text_color = Some(self.text_primary);
        visuals.panel_fill = self.canvas;
        visuals.window_fill = self.popover_bg;
        visuals.faint_bg_color = self.glass_surface;
        visuals.extreme_bg_color = Color32::from_rgb(0x07, 0x09, 0x0D);
        visuals.code_bg_color = Color32::from_rgb(0x0C, 0x10, 0x17);
        visuals.window_stroke = Stroke::new(1.0, self.hairline_strong);
        visuals.window_rounding = Rounding::same(self.rounding_md);
        visuals.menu_rounding = Rounding::same(self.rounding_md);
        visuals.selection.bg_fill = self.gold_fill;
        visuals.selection.stroke = Stroke::new(1.0, self.gold);
        visuals.hyperlink_color = self.teal;
        visuals.warn_fg_color = self.gold;
        visuals.error_fg_color = self.danger;

        let r_sm = Rounding::same(self.rounding_sm);

        visuals.widgets.noninteractive.bg_fill = self.canvas;
        visuals.widgets.noninteractive.weak_bg_fill = self.canvas;
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, self.hairline);
        visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, self.text_secondary);
        visuals.widgets.noninteractive.rounding = r_sm;

        visuals.widgets.inactive.bg_fill = self.glass_surface;
        visuals.widgets.inactive.weak_bg_fill = self.glass_surface;
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, self.hairline);
        visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, self.text_primary);
        visuals.widgets.inactive.rounding = r_sm;

        visuals.widgets.hovered.bg_fill = self.glass_hover;
        visuals.widgets.hovered.weak_bg_fill = self.glass_hover;
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, self.hairline_strong);
        visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, self.text_primary);
        visuals.widgets.hovered.rounding = r_sm;

        visuals.widgets.active.bg_fill = self.gold_tint;
        visuals.widgets.active.weak_bg_fill = self.gold_tint;
        visuals.widgets.active.bg_stroke = Stroke::new(1.0, self.gold);
        visuals.widgets.active.fg_stroke = Stroke::new(1.0, self.text_primary);
        visuals.widgets.active.rounding = r_sm;

        let mut style = Style {
            visuals,
            ..Style::default()
        };

        style.spacing.item_spacing = egui::vec2(12.0, 12.0);
        style.spacing.button_padding = egui::vec2(14.0, 7.0);
        style.spacing.window_margin = egui::Margin::same(16.0);
        style.spacing.interact_size = egui::vec2(40.0, 30.0);

        style.text_styles.insert(
            TextStyle::Heading,
            FontId::new(19.0, FontFamily::Proportional),
        );
        style
            .text_styles
            .insert(TextStyle::Body, FontId::new(13.5, FontFamily::Proportional));
        style.text_styles.insert(
            TextStyle::Button,
            FontId::new(13.0, FontFamily::Proportional),
        );
        style
            .text_styles
            .insert(TextStyle::Small, FontId::new(11.5, FontFamily::Proportional));
        style.text_styles.insert(
            TextStyle::Monospace,
            FontId::new(12.0, FontFamily::Monospace),
        );

        ctx.set_style(style);
    }

    /// Configures typography for the launcher. Currently falls back to egui's
    /// built-in proportional and monospace fonts so builds succeed without
    /// external font asset files.
    fn install_fonts(&self, ctx: &Context) {
        let fonts = FontDefinitions::default();
        ctx.set_fonts(fonts);
    }

    pub fn popover_frame(&self) -> egui::Frame {
        egui::Frame::none()
            .fill(self.popover_bg)
            .stroke(Stroke::new(1.0, self.hairline_strong))
            .rounding(Rounding::same(self.rounding_md))
            .inner_margin(egui::Margin::symmetric(12.0, 10.0))
    }

    /// Draws a crisp 1px horizontal hairline rule spanning the available width.
    pub fn hairline(&self, ui: &mut Ui) {
        let (rect, _) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), 1.0),
            Sense::hover(),
        );
        ui.painter().hline(
            rect.x_range(),
            rect.center().y,
            Stroke::new(1.0, self.hairline),
        );
    }

    /// Renders a muted 14px information-circle icon that reveals an elevated
    /// translucent tooltip popover on hover.
    pub fn info_tooltip(&self, ui: &mut Ui, title: &str, body: &str) {
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
        let style = IconStyle::from_active(response.hovered());
        let tint = if response.hovered() {
            self.text_primary
        } else {
            self.text_muted
        };
        let icon_rect = Rect::from_center_size(rect.center(), Vec2::splat(14.0));
        icon_image(IconKind::InformationCircle, style, 14.0, tint).paint_at(ui, icon_rect);

        response.on_hover_ui(|ui| {
            self.popover_frame().show(ui, |ui| {
                ui.set_max_width(260.0);
                ui.label(
                    egui::RichText::new(title)
                        .size(12.0)
                        .strong()
                        .color(self.text_primary),
                );
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(body)
                        .size(11.5)
                        .color(self.text_secondary),
                );
            });
        });
    }

    /// macOS Control Center-inspired 28px pill track slider with frosted glass fill,
    /// warm gold progress illumination, and subtle spring expansion while dragging.
    pub fn glass_slider_u32(
        &self,
        ui: &mut Ui,
        label: &str,
        value: &mut u32,
        min: u32,
        max: u32,
        step: u32,
        value_text: &str,
    ) -> Response {
        let desired = Vec2::new(ui.available_width(), 28.0);
        let (rect, mut response) = ui.allocate_exact_size(desired, Sense::click_and_drag());

        if response.clicked() || response.dragged() {
            if let Some(pointer) = response.interact_pointer_pos() {
                let t = ((pointer.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
                let raw = min as f32 + t * (max - min) as f32;
                let snapped = (((raw - min as f32) / step as f32).round() as u32)
                    .saturating_mul(step)
                    .saturating_add(min)
                    .clamp(min, max);
                if snapped != *value {
                    *value = snapped;
                    response.mark_changed();
                }
            }
        }

        let stretch = ui
            .ctx()
            .animate_bool(response.id.with("drag"), response.dragged());
        let visual_rect = rect.expand2(Vec2::new(stretch * 2.0, stretch * 1.5));
        let radius = Rounding::same(visual_rect.height() * 0.5);
        let painter = ui.painter();

        // Frosted glass pill track
        painter.rect(
            visual_rect,
            radius,
            Color32::from_rgba_unmultiplied(255, 255, 255, 15),
            Stroke::new(
                1.0,
                if response.hovered() || response.dragged() {
                    self.hairline_strong
                } else {
                    self.hairline
                },
            ),
        );

        // Warm gold illuminated fill bar
        let frac = ((*value - min) as f32 / (max - min).max(1) as f32).clamp(0.0, 1.0);
        let fill_width = (visual_rect.width() * frac).max(visual_rect.height());
        let fill_rect = Rect::from_min_size(
            visual_rect.min,
            Vec2::new(fill_width.min(visual_rect.width()), visual_rect.height()),
        );
        painter.rect_filled(fill_rect, radius, self.gold_fill);

        // Label (left) and crisp tabular value (right)
        painter.text(
            Pos2::new(visual_rect.left() + 12.0, visual_rect.center().y),
            Align2::LEFT_CENTER,
            label,
            FontId::new(12.5, FontFamily::Proportional),
            self.text_primary,
        );
        painter.text(
            Pos2::new(visual_rect.right() - 12.0, visual_rect.center().y),
            Align2::RIGHT_CENTER,
            value_text,
            FontId::new(12.0, FontFamily::Monospace),
            self.gold,
        );

        response
    }

    /// Razor-thin 3px progress bar without heavy pill framing.
    pub fn thin_progress_bar(&self, ui: &mut Ui, fraction: f32, active_color: Color32) {
        let (rect, _) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), 3.0),
            Sense::hover(),
        );
        let rounding = Rounding::same(1.5);
        let painter = ui.painter();
        painter.rect_filled(
            rect,
            rounding,
            Color32::from_rgba_unmultiplied(255, 255, 255, 18),
        );
        let clamped = fraction.clamp(0.0, 1.0);
        if clamped > 0.0 {
            let fill_rect = Rect::from_min_size(
                rect.min,
                Vec2::new(rect.width() * clamped, rect.height()),
            );
            painter.rect_filled(fill_rect, rounding, active_color);
        }
    }
}
