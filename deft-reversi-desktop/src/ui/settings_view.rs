//! The settings screen (§9.6).
use super::{root::RootView, theme, widgets};
use crate::{
    app::{AnalysisSettings, Command},
    domain::settings::{Animation, TT_SIZES_MIB, ThemeChoice},
    i18n::Lang,
    services::engine::{BookStatus, Resources},
};
use deft_reversi_book::AnalysisIdentity;
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
            // Indented under the group title.
            widgets::form_row(170., label, control, note).pl(px(14.))
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

        let analysis = |change: fn(&mut AnalysisSettings)| {
            widgets::handler(move |this, cx| {
                let mut analysis = this.settings.analysis;
                change(&mut analysis);
                if analysis != this.settings.analysis {
                    this.settings.analysis = analysis;
                    this.run(Command::SetAnalysisSettings(analysis), cx);
                }
            })
        };
        let candidate_values = widgets::toggle(
            "candidate-values",
            s.show,
            s.hide,
            settings.candidate_values,
            |this, on, _| this.settings.candidate_values = on,
            cx,
        );
        let level = widgets::stepper(
            "analysis-level",
            format!("Lv {}", settings.analysis.level),
            analysis(|a| a.level = a.level.saturating_sub(1).max(1)),
            analysis(|a| a.level = (a.level + 1).min(60)),
            cx,
        );
        let loss = widgets::stepper(
            "loss-threshold",
            settings.analysis.loss_threshold.to_string(),
            analysis(|a| a.loss_threshold = (a.loss_threshold - 1).max(1)),
            analysis(|a| a.loss_threshold = (a.loss_threshold + 1).min(64)),
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
            .child(widgets::toggle(
                "sound",
                s.on,
                s.off,
                settings.sound,
                |this, on, _| this.settings.sound = on,
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

        let legal = widgets::toggle(
            "legal-moves",
            s.show,
            s.hide,
            settings.show_legal_moves,
            |this, on, _| this.settings.show_legal_moves = on,
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
            widgets::handler(move |this, cx| {
                let mut resources = this.settings.resources;
                resources.threads = (resources.threads + 1).min(cpus);
                if resources != this.settings.resources {
                    this.settings.resources = resources;
                    this.run(Command::SetResources(resources), cx);
                }
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
            (Some(_), Some(BookStatus::Loaded { positions, .. })) => {
                Some(s.book_loaded(*positions).into())
            }
            (Some(_), Some(BookStatus::OtherModel)) => Some(s.book_other_model.into()),
            (Some(_), Some(BookStatus::Failed(error))) => {
                Some(format!("{}: {error}", s.book_failed).into())
            }
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
        let book_use = widgets::toggle(
            "book-use",
            s.on,
            s.off,
            book.enabled,
            |this, on, cx| this.set_book_use(on, cx),
            cx,
        );

        let body =
            div()
                .flex()
                .flex_col()
                .child(group(s.board_group))
                .child(row(s.legal_moves, legal, None))
                .child(row(s.candidate_values, candidate_values, None))
                .child(row(s.flip_animation, animation, None))
                .child(row(s.sound, sound, None))
                .child(group(s.analysis))
                .child(row(s.analysis_level, level, None))
                .child(div().pl(px(14. + 170. + 12.)).pb(px(8.)).child(
                    super::new_game::level_chart(s, settings.analysis.level, 400.),
                ))
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
                .child(group(s.version_group))
                .children(self.version_rows());

        // The title stays; the rest scrolls when the window is short.
        widgets::dialog_box(680.)
            // The window size from the last frame; the title stays on screen.
            .max_h(px(self.viewport.map_or(900., |(_, h)| h as f32 - 60.)))
            .child(widgets::dialog_header(
                s.settings,
                Some(("close-settings", s.close)),
                cx,
            ))
            .child(widgets::scroll_area(
                "settings-body",
                &self.dialog_scroll,
                body,
                cx,
            ))
    }

    /// The app, the engine, the book library, the evaluation model and the
    /// book in use, each with what tells one build or file from another.
    fn version_rows(&self) -> Vec<Div> {
        let s = self.settings.lang.strings();
        let id = |identity: &AnalysisIdentity| {
            let hex: String = identity[..6].iter().map(|b| format!("{b:02x}")).collect();
            format!("{}: {hex}", s.analysis_id)
        };
        let row = |label: &'static str, value: SharedString, details: Vec<String>| {
            div()
                .flex()
                .flex_row()
                .items_start()
                .gap(px(12.))
                .pl(px(14.))
                .py(px(5.))
                .child(div().w(px(170.)).flex_none().child(label))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(1.))
                        .child(value)
                        .children(details.into_iter().map(|line| {
                            widgets::caption(line)
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                        })),
                )
        };

        let model = match &self.model_info {
            Some(info) => {
                let name = info.path.file_name().map_or_else(
                    || info.path.display().to_string(),
                    |n| n.to_string_lossy().into(),
                );
                let mut details = vec![
                    s.model_format(info.format, info.calibrated),
                    format!("{:.1} MiB", info.bytes as f64 / (1024. * 1024.)),
                ];
                details.extend(info.identity.as_ref().map(id));
                details.push(info.path.display().to_string());
                row(s.version_model, name.into(), details)
            }
            None => row(s.version_model, s.version_not_loaded.into(), Vec::new()),
        };

        let book = match (&self.settings.book.path, &self.book_status) {
            (Some(path), Some(BookStatus::Loaded { book, .. })) => {
                let book = &book.0;
                let name = path.file_name().map_or_else(
                    || path.display().to_string(),
                    |n| n.to_string_lossy().into(),
                );
                let mut details = vec![format!("{}: {}", s.book_written_by, book.producer())];
                details.extend(
                    book.generation_runs()
                        .last()
                        .map(|run| format!("{}: {}", s.book_grown_with, run.engine_version)),
                );
                details.extend(
                    book.analysis_profiles()
                        .find(|p| p.identity == book.identity())
                        .and_then(|p| p.model_label.as_ref())
                        .map(|label| format!("{}: {label}", s.book_model_label)),
                );
                details.push(id(&book.identity()));
                Some(row(s.book, name.into(), details))
            }
            _ => None,
        };

        [
            row(
                s.version_app,
                format!("deft-reversi {}", env!("CARGO_PKG_VERSION")).into(),
                vec![format!("git {}", deft_reversi_engine::GIT_REVISION)],
            ),
            row(
                s.engine_group,
                deft_reversi_engine::VERSION.into(),
                vec![s.search_revision(deft_reversi_engine::SEARCH_SEMANTICS_REVISION)],
            ),
            row(
                s.version_book_lib,
                deft_reversi_book::VERSION.into(),
                Vec::new(),
            ),
            model,
        ]
        .into_iter()
        .chain(book)
        .collect()
    }
}
