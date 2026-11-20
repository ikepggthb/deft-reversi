//! The settings screen (§9.6).
use super::{root::RootView, theme, widgets};
use crate::{
    app::{AnalysisSettings, Command},
    domain::settings::{Animation, TT_SIZES_MIB, ThemeChoice},
    i18n::Lang,
    services::engine::{BookStatus, Resources},
};
use gpui::{Context, Div, FontWeight, IntoElement, SharedString, div, prelude::*, px};

impl RootView {
    pub(super) fn render_settings(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let s = self.settings.lang.strings();
        let settings = self.settings.clone();
        let cpus = std::thread::available_parallelism().map_or(1, |n| n.get());

        let group = |title: &'static str| {
            div()
                .mt(px(14.))
                .mb(px(2.))
                .pt(px(10.))
                .border_t_1()
                .border_color(theme::hairline())
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
                .gap(px(12.))
                .min_h(px(34.))
                // Indented under the group title.
                .pl(px(14.))
                .child(div().w(px(170.)).flex_none().child(label))
                .child(control.flex_none())
                .children(note.map(|n| {
                    div()
                        .text_xs()
                        .text_color(theme::text_muted())
                        .whitespace_nowrap()
                        .child(n)
                }))
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

        let analysis = |change: Box<dyn Fn(&mut AnalysisSettings)>| {
            widgets::handler(move |this, cx| {
                let mut analysis = this.settings.analysis;
                change(&mut analysis);
                this.settings.analysis = analysis;
                this.run(Command::SetAnalysisSettings(analysis), cx);
            })
        };
        let candidate_values = widgets::segmented(
            "candidate-values",
            vec![
                (
                    s.show.into(),
                    settings.candidate_values,
                    widgets::handler(|this, _| this.settings.candidate_values = true),
                ),
                (
                    s.hide.into(),
                    !settings.candidate_values,
                    widgets::handler(|this, _| this.settings.candidate_values = false),
                ),
            ],
            cx,
        );
        let level = widgets::stepper(
            "analysis-level",
            format!("Lv {}", settings.analysis.level),
            analysis(Box::new(|a| a.level = a.level.saturating_sub(1).max(1))),
            analysis(Box::new(|a| a.level = (a.level + 1).min(60))),
            cx,
        );
        let loss = widgets::stepper(
            "loss-threshold",
            settings.analysis.loss_threshold.to_string(),
            analysis(Box::new(|a| {
                a.loss_threshold = (a.loss_threshold - 1).max(1)
            })),
            analysis(Box::new(|a| {
                a.loss_threshold = (a.loss_threshold + 1).min(64)
            })),
            cx,
        );

        let theme_choice = widgets::segmented(
            "theme",
            [
                (ThemeChoice::System, s.theme_system),
                (ThemeChoice::Light, s.theme_light),
                (ThemeChoice::Dark, s.theme_dark),
            ]
            .into_iter()
            .map(|(choice, label)| {
                (
                    SharedString::from(label),
                    settings.theme == choice,
                    widgets::handler(move |this, _| this.settings.theme = choice),
                )
            })
            .collect(),
            cx,
        );
        let animation = widgets::segmented(
            "animation",
            [
                (Animation::Normal, s.animation_normal),
                (Animation::Short, s.animation_short),
                (Animation::Off, s.animation_off),
            ]
            .into_iter()
            .map(|(choice, label)| {
                (
                    SharedString::from(label),
                    settings.animation == choice,
                    widgets::handler(move |this, _| this.settings.animation = choice),
                )
            })
            .collect(),
            cx,
        );
        let sound = div()
            .flex()
            .flex_row()
            .gap(px(10.))
            .child(widgets::segmented(
                "sound",
                vec![
                    (
                        s.on.into(),
                        settings.sound,
                        widgets::handler(|this, _| this.settings.sound = true),
                    ),
                    (
                        s.off.into(),
                        !settings.sound,
                        widgets::handler(|this, _| this.settings.sound = false),
                    ),
                ],
                cx,
            ))
            .when(settings.sound, |el| {
                el.child(widgets::stepper(
                    "volume",
                    settings.volume.to_string(),
                    widgets::handler(|this, _| {
                        this.settings.volume = this.settings.volume.saturating_sub(10)
                    }),
                    widgets::handler(|this, _| {
                        this.settings.volume = (this.settings.volume + 10).min(100)
                    }),
                    cx,
                ))
            });

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

        let book = settings.book.clone();
        let book_name = book.path.as_ref().map(|p| {
            p.file_name()
                .map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into())
        });
        let book_state: Option<SharedString> = match (&book.path, &self.book_status) {
            (None, _) => None,
            (Some(_), None) => Some(s.book_loading.into()),
            (Some(_), Some(BookStatus::Loaded { positions })) => {
                Some(s.book_loaded(*positions).into())
            }
            (Some(_), Some(BookStatus::OtherModel)) => Some(s.book_other_model.into()),
            (Some(_), Some(BookStatus::Failed(_))) => Some(s.book_failed.into()),
            (Some(_), Some(BookStatus::None)) => None,
        };
        let book_file = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(10.))
            .child(
                div()
                    .max_w(px(200.))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_color(if book_name.is_some() {
                        theme::text()
                    } else {
                        theme::text_muted()
                    })
                    .child(book_name.unwrap_or_else(|| s.book_none.into())),
            )
            .child(widgets::button(
                "book-choose",
                s.book_choose,
                widgets::handler(|this, cx| this.choose_book(cx)),
                cx,
            ))
            .when(book.path.is_some(), |el| {
                el.child(widgets::link(
                    "book-remove",
                    s.book_remove,
                    widgets::handler(|this, cx| this.set_book_path(None, cx)),
                    cx,
                ))
            });
        let book_use = widgets::segmented(
            "book-use",
            [(true, s.on), (false, s.off)]
                .into_iter()
                .map(|(on, label)| {
                    (
                        SharedString::from(label),
                        book.enabled == on,
                        widgets::handler(move |this, cx| this.set_book_use(on, cx)),
                    )
                })
                .collect(),
            cx,
        );

        let body = div()
            .flex()
            .flex_col()
            .child(group(s.board_group))
            .child(row(s.legal_moves, legal, None))
            .child(row(s.candidate_values, candidate_values, None))
            .child(row(s.flip_animation, animation, None))
            .child(row(s.sound, sound, None))
            .child(group(s.analysis))
            .child(row(s.analysis_level, level, None))
            .child(row(s.loss_threshold, loss, Some(s.discs_or_more.into())))
            .child(group(s.appearance))
            .child(row(s.theme, theme_choice, None))
            .child(row(s.language, language, None))
            .child(group(s.book_group))
            .child(row(s.book_file, book_file, book_state))
            .child(row(s.book_use, book_use, None))
            .child(
                div()
                    .pl(px(14.))
                    .py(px(4.))
                    .text_xs()
                    .text_color(theme::text_muted())
                    .child(s.book_note),
            )
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
            );

        // The title stays; the rest scrolls when the window is short.
        div()
            .w(px(680.))
            // The window size from the last frame; the title stays on screen.
            .max_h(px(self.viewport.map_or(900., |(_, h)| h as f32 - 60.)))
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
            .child(widgets::scroll_area(
                "settings-body",
                &self.settings_scroll,
                body,
                cx,
            ))
    }
}
