//! The settings screen (§9.6). It lists only what works so far; the
//! analysis, theme, animation and sound settings come with those features.
use super::{root::RootView, theme, widgets};
use crate::{
    app::Command, domain::settings::TT_SIZES_MIB, i18n::Lang, services::engine::Resources,
};
use gpui::{Context, Div, FontWeight, IntoElement, SharedString, div, prelude::*, px};

impl RootView {
    pub(super) fn render_settings(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let s = self.settings.lang.strings();
        let settings = self.settings;
        let cpus = std::thread::available_parallelism().map_or(1, |n| n.get());

        let group = |title: &'static str| {
            div()
                .mt(px(14.))
                .mb(px(2.))
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme::text_secondary())
                .child(title)
        };
        let row = |label: &'static str, control: Div, note: Option<SharedString>| {
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(10.))
                .min_h(px(32.))
                .child(div().w(px(110.)).child(label))
                .child(control)
                .children(note.map(|n| div().text_xs().text_color(theme::text_muted()).child(n)))
        };
        let resources = move |change: fn(&mut Resources)| {
            widgets::handler(move |this, cx| {
                let mut resources = this.settings.resources;
                change(&mut resources);
                if resources != this.settings.resources {
                    this.settings.resources = resources;
                    this.run(Command::SetResources(resources), cx);
                }
            })
        };

        let legal = widgets::segmented(
            "legal-moves",
            vec![
                (
                    s.show.into(),
                    settings.show_legal_moves,
                    widgets::handler(|this, _| this.settings.show_legal_moves = true),
                ),
                (
                    s.hide.into(),
                    !settings.show_legal_moves,
                    widgets::handler(|this, _| this.settings.show_legal_moves = false),
                ),
            ],
            cx,
        );
        let language = widgets::segmented(
            "language",
            vec![
                (
                    "日本語".into(),
                    settings.lang == Lang::Ja,
                    widgets::handler(|this, _| this.settings.lang = Lang::Ja),
                ),
                (
                    "English".into(),
                    settings.lang == Lang::En,
                    widgets::handler(|this, _| this.settings.lang = Lang::En),
                ),
            ],
            cx,
        );
        let threads = widgets::stepper(
            "threads",
            settings.resources.threads.to_string(),
            resources(|r| r.threads = r.threads.saturating_sub(1).max(1)),
            resources(|r| {
                let cpus = std::thread::available_parallelism().map_or(1, |n| n.get());
                r.threads = (r.threads + 1).min(cpus)
            }),
            cx,
        );
        let tt = widgets::segmented(
            "hash-table",
            TT_SIZES_MIB
                .iter()
                .map(|&mib| {
                    (
                        SharedString::from(mib.to_string()),
                        settings.resources.tt_mib == mib,
                        widgets::handler(move |this, cx| {
                            let mut resources = this.settings.resources;
                            resources.tt_mib = mib;
                            if resources != this.settings.resources {
                                this.settings.resources = resources;
                                this.run(Command::SetResources(resources), cx);
                            }
                        }),
                    )
                })
                .collect(),
            cx,
        );

        div()
            .w(px(520.))
            .flex()
            .flex_col()
            .px(px(20.))
            .py(px(16.))
            .bg(theme::surface())
            .border_1()
            .border_color(theme::border())
            .rounded(px(10.))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .text_base()
                            .font_weight(FontWeight::MEDIUM)
                            .child(s.settings),
                    )
                    .child(widgets::link(
                        "close-settings",
                        s.close,
                        widgets::handler(|this, cx| this.close_dialog(cx)),
                        cx,
                    )),
            )
            .child(group(s.board_group))
            .child(row(s.legal_moves, legal, None))
            .child(group(s.appearance))
            .child(row(s.language, language, None))
            .child(group(s.engine_group))
            .child(row(s.threads, threads, Some(s.logical_cpus(cpus).into())))
            .child(row(s.hash_table, tt, Some("MiB".into())))
            .child(
                div()
                    .mt(px(14.))
                    .text_xs()
                    .text_color(theme::text_muted())
                    .child(format!(
                        "{} · deft-reversi {}",
                        crate::engine_loader::MODEL_FILE.trim_end_matches(".dpr2"),
                        env!("CARGO_PKG_VERSION")
                    )),
            )
    }
}
