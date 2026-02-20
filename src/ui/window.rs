use log::{error, warn};

use crate::app::AppState;
use crate::clipboard::ClipboardMonitor;
use crate::icons;
use crate::models::EntryMetadata;
use crate::settings;
use crate::storage::Storage;
use crate::ui::components::animated_icon_svg;
use crate::ui::theme::{
    BORDER_RADIUS_SM, BORDER_WIDTH, COPY_BUTTON_OFFSET, COPY_BUTTON_SIZE, FONT_SIZE_BASE,
    FONT_SIZE_SM, FONT_SIZE_XS, HEADER_CLEARANCE_HEIGHT, IconSize, KopiColors, KopiStyleExt,
    SIDEBAR_ITEM_HEIGHT, SIDEBAR_SECTION_HEIGHT, SPACING_LG, SPACING_MD, SPACING_SM, SPACING_XL,
    SPACING_XS, get_kopi_colors,
};

use gpui::{
    Animation, AnimationExt, AnyElement, Context, CursorStyle, Div, Entity, FocusHandle, Hsla,
    IntoElement, Render, SharedString, Subscription, Timer, Window, actions, div, ease_in_out,
    prelude::*, svg,
};
use gpui_component::Sizable;
use gpui_component::StyledExt;
use gpui_component::Theme;
use gpui_component::input::{Input, InputEvent, InputState, SelectAll};
use gpui_component::theme::ActiveTheme;

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};
use uuid::Uuid;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum SidebarButtonKind {
    Pencil,
    Star,
    Trash,
}

impl SidebarButtonKind {
    const fn prefix(self) -> &'static str {
        match self {
            Self::Pencil => "pencil",
            Self::Star => "star",
            Self::Trash => "trash",
        }
    }

    fn id_for(self, entry_id: Uuid) -> SharedString {
        SharedString::from(format!("{}-{}", self.prefix(), entry_id))
    }

    fn anim_id_for(self, entry_id: Uuid) -> String {
        format!("{}-icon-anim-{}", self.prefix(), entry_id)
    }
}

actions!(kopi, [CancelTitleEdit, DeleteSelectedEntries]);

const EDITOR_SAVE_DEBOUNCE_MS: u64 = 300;
const COPY_ANIMATION_SCALE_MIN: f32 = 0.8;
const COPY_ANIMATION_SCALE_DELTA: f32 = 0.4;
const TITLE_EDIT_GRACE_PERIOD_MS: u64 = 50;

pub struct EditorState {
    pub state: Entity<InputState>,
    pub entry_id: Option<Uuid>,
    pub save_pending: bool,
    pub pending_content_update: Option<(Uuid, String)>,
    pub save_generation: u64,
}

pub struct TitleEditState {
    pub editing_entry_id: Option<Uuid>,
    pub input_state: Entity<InputState>,
    pub original_title: Option<String>,
    pub cancelled: bool,
    pub started_at: Option<Instant>,
}

pub struct KopiWindow {
    pub app_state: AppState,
    clipboard_monitor: Arc<ClipboardMonitor>,
    show_copied_tooltip: bool,
    editor: EditorState,
    title_edit: TitleEditState,
    search_input_state: Entity<InputState>,
    focus_handle: FocusHandle,
    hovered_icons: HashSet<SharedString>,
    skip_blur: bool,
    #[allow(dead_code)]
    appearance_subscription: Subscription,
    #[allow(dead_code)]
    bounds_subscription: Subscription,
    #[allow(dead_code)]
    activation_subscription: Subscription,
}

impl KopiWindow {
    pub fn new(
        storage: Arc<Storage>,
        clipboard_monitor: Arc<ClipboardMonitor>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let app_state = AppState::new(storage);

        let editor_input_state = cx.new(|cx| InputState::new(window, cx).multi_line());

        let title_input_state = cx.new(|cx| InputState::new(window, cx));

        let search_input_state = cx.new(|cx| InputState::new(window, cx));

        let focus_handle = cx.focus_handle();
        focus_handle.focus(window);

        let editor_entry_id = app_state.selected_entry_id;
        if let Some(content) = app_state.selected_entry_content() {
            editor_input_state.update(cx, |state, cx| {
                state.set_value(content.to_string(), window, cx);
            });
        }

        cx.subscribe(&editor_input_state, Self::on_editor_change)
            .detach();
        cx.subscribe(&title_input_state, Self::on_title_input_event)
            .detach();
        cx.subscribe(&search_input_state, Self::on_search_input_change)
            .detach();

        let appearance_subscription = cx.observe_window_appearance(window, |_this, window, cx| {
            Theme::sync_system_appearance(Some(window), cx);
            cx.notify();
        });

        let bounds_subscription = cx.observe_window_bounds(window, |_this, window, _cx| {
            let bounds = window.bounds();
            let fullscreen = window.is_fullscreen();
            let state = settings::WindowState::from_bounds(bounds, fullscreen);
            settings::save_window_state(&state);
        });

        let activation_subscription =
            cx.observe_window_activation(window, |this: &mut KopiWindow, window, _cx| {
                let active = window.is_window_active();
                this.clipboard_monitor.set_window_active(active);
            });

        let editor = EditorState {
            state: editor_input_state,
            entry_id: editor_entry_id,
            save_pending: false,
            pending_content_update: None,
            save_generation: 0,
        };

        let title_edit = TitleEditState {
            editing_entry_id: None,
            input_state: title_input_state,
            original_title: None,
            cancelled: false,
            started_at: None,
        };

        Self {
            app_state,
            clipboard_monitor,
            show_copied_tooltip: false,
            editor,
            title_edit,
            search_input_state,
            focus_handle,
            hovered_icons: HashSet::new(),
            skip_blur: false,
            appearance_subscription,
            bounds_subscription,
            activation_subscription,
        }
    }

    fn on_editor_change(
        &mut self,
        _: Entity<InputState>,
        event: &InputEvent,
        cx: &mut Context<Self>,
    ) {
        if let InputEvent::Change = event
            && let Some(entry_id) = self.editor.entry_id
        {
            let content = self.editor.state.read(cx).text().to_string();

            self.editor.pending_content_update = Some((entry_id, content));
            self.editor.save_pending = true;

            self.editor.save_generation = self.editor.save_generation.wrapping_add(1);
            let current_generation = self.editor.save_generation;
            let debounce_entry_id = entry_id;

            cx.spawn(async move |this, cx| {
                Timer::after(Duration::from_millis(EDITOR_SAVE_DEBOUNCE_MS)).await;
                if let Err(e) = this.update(cx, |this: &mut KopiWindow, cx| {
                    if this.editor.save_generation == current_generation {
                        this.flush_pending_content_update(Some(debounce_entry_id), cx);
                    }
                }) {
                    warn!("Failed to flush pending content update: {:?}", e);
                }
            })
            .detach();

            cx.notify();
        }
    }

    fn flush_pending_content_update(
        &mut self,
        expected_entry_id: Option<Uuid>,
        cx: &mut Context<Self>,
    ) {
        if let Some((entry_id, content)) = self.editor.pending_content_update.take() {
            if expected_entry_id.is_none() || expected_entry_id == Some(entry_id) {
                self.app_state.update_entry_content(entry_id, content);
                self.editor.save_pending = false;
                cx.notify();
            } else {
                self.editor.pending_content_update = Some((entry_id, content));
            }
        }
    }

    fn on_title_input_event(
        &mut self,
        _: Entity<InputState>,
        event: &InputEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::PressEnter { .. } => {
                cx.spawn(async move |this, cx| {
                    if let Err(e) = this.update(cx, |this: &mut KopiWindow, cx| {
                        this.save_title_edit_deferred(cx);
                    }) {
                        warn!("Failed to save title edit on enter: {:?}", e);
                    }
                })
                .detach();
            }
            InputEvent::Blur => {
                if !self.title_edit.cancelled && self.title_edit.editing_entry_id.is_some() {
                    cx.spawn(async move |this, cx| {
                        if let Err(e) = this.update(cx, |this: &mut KopiWindow, cx| {
                            this.save_title_edit_deferred(cx);
                        }) {
                            warn!("Failed to save title edit on blur: {:?}", e);
                        }
                    })
                    .detach();
                }
                self.title_edit.cancelled = false;
            }
            _ => {}
        }
    }

    fn on_search_input_change(
        &mut self,
        _: Entity<InputState>,
        event: &InputEvent,
        cx: &mut Context<Self>,
    ) {
        if let InputEvent::Change = event {
            let query = self.search_input_state.read(cx).text().to_string();
            self.app_state.set_search_query(query);
            self.clear_all_entry_hover_states();
            cx.notify();
        }
    }

    fn is_within_title_edit_grace_period(&self) -> bool {
        if let Some(started_at) = self.title_edit.started_at {
            started_at.elapsed() < Duration::from_millis(TITLE_EDIT_GRACE_PERIOD_MS)
        } else {
            false
        }
    }

    fn save_title_edit_deferred(&mut self, cx: &mut Context<Self>) {
        if self.is_within_title_edit_grace_period() {
            return;
        }

        if let Some(entry_id) = self.title_edit.editing_entry_id.take() {
            self.title_edit.started_at = None;
            let new_title = self.title_edit.input_state.read(cx).text().to_string();
            let trimmed = new_title.trim();

            if trimmed.is_empty() {
                self.app_state.reset_entry_title(entry_id);
            } else {
                let truncated: String = trimmed.chars().take(50).collect();
                self.app_state.update_entry_title(entry_id, truncated);
            }

            self.title_edit.original_title = None;
            cx.notify();
        }
    }

    fn start_title_edit(&mut self, entry_id: Uuid, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(entry) = self.app_state.entries.get(&entry_id) {
            let current_title = entry.title.clone();
            self.title_edit.original_title = Some(current_title.clone());
            self.title_edit.editing_entry_id = Some(entry_id);
            self.title_edit.cancelled = false;
            self.title_edit.started_at = Some(Instant::now());

            self.app_state.select_entry(entry_id);
            self.sync_editor_to_selection(window, cx);

            self.title_edit.input_state.update(cx, |state, cx| {
                state.set_value(&current_title, window, cx);
            });

            self.title_edit.input_state.update(cx, |state, cx| {
                state.focus(window, cx);
            });

            window.dispatch_action(Box::new(SelectAll), cx);

            cx.notify();
        }
    }

    fn save_title_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_within_title_edit_grace_period() {
            return;
        }

        if let Some(entry_id) = self.title_edit.editing_entry_id.take() {
            self.title_edit.started_at = None;
            let new_title = self.title_edit.input_state.read(cx).text().to_string();
            let trimmed = new_title.trim();

            if trimmed.is_empty() {
                self.app_state.reset_entry_title(entry_id);
            } else {
                let truncated: String = trimmed.chars().take(50).collect();
                self.app_state.update_entry_title(entry_id, truncated);
            }

            self.title_edit.original_title = None;

            self.editor.state.update(cx, |state, cx| {
                state.focus(window, cx);
            });

            cx.notify();
        }
    }

    fn cancel_title_edit(&mut self, cx: &mut Context<Self>) {
        self.title_edit.editing_entry_id = None;
        self.title_edit.original_title = None;
        self.title_edit.started_at = None;
        self.title_edit.cancelled = true;
        cx.notify();
    }

    fn clear_entry_hover_state(&mut self, entry_id: Uuid) {
        self.hovered_icons
            .remove(&SidebarButtonKind::Star.id_for(entry_id));
        self.hovered_icons
            .remove(&SidebarButtonKind::Trash.id_for(entry_id));
        self.hovered_icons
            .remove(&SidebarButtonKind::Pencil.id_for(entry_id));
    }

    fn clear_all_entry_hover_states(&mut self) {
        self.hovered_icons
            .retain(|id| id.as_ref() == "copy-button" || id.as_ref() == "sidebar-toggle");
    }

    fn handle_cancel_title_edit(
        &mut self,
        _: &CancelTitleEdit,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.title_edit.editing_entry_id.is_some() {
            self.cancel_title_edit(cx);
        } else {
            cx.propagate();
        }
    }

    fn handle_delete_selected(
        &mut self,
        _: &DeleteSelectedEntries,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let deleted_ids = self.app_state.soft_delete_selected();
        if deleted_ids.is_empty() {
            return;
        }

        for &entry_id in &deleted_ids {
            self.clear_entry_hover_state(entry_id);
        }

        self.sync_editor_to_selection(_window, cx);

        for entry_id in deleted_ids {
            let entity = cx.entity().clone();
            _window
                .spawn(cx, async move |cx| {
                    Timer::after(std::time::Duration::from_secs(5)).await;
                    if let Err(e) = entity.update(cx, |this: &mut KopiWindow, cx| {
                        if this.app_state.is_pending_delete(entry_id) {
                            this.app_state.clear_pending_delete(entry_id);
                            cx.notify();
                        }
                    }) {
                        warn!("Failed to clear pending delete: {:?}", e);
                    }
                })
                .detach();
        }

        cx.notify();
    }

    fn handle_toggle_capture(
        &mut self,
        _: &crate::ToggleCaptureEditorCopies,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let new_val = !self.clipboard_monitor.capture_editor_copies();
        self.clipboard_monitor.set_capture_editor_copies(new_val);
        settings::save_app_settings(&settings::AppSettings {
            capture_editor_copies: new_val,
        });
        crate::rebuild_menus(cx, new_val);
        cx.notify();
    }

    pub fn sync_editor_to_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let new_id = self.app_state.selected_entry_id;

        if new_id != self.editor.entry_id {
            self.flush_pending_content_update(None, cx);

            self.editor.entry_id = new_id;
            self.editor.save_pending = false;

            if let Some(content) = self.app_state.selected_entry_content() {
                self.editor.state.update(cx, |state, cx| {
                    state.set_value(content.to_string(), window, cx);
                });
            } else {
                self.editor.state.update(cx, |state, cx| {
                    state.set_value("", window, cx);
                });
            }

            cx.notify();
        }
    }

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = get_kopi_colors(cx);

        div()
            .h(HEADER_CLEARANCE_HEIGHT)
            .w_full()
            .bg(colors.background)
    }

    fn render_copy_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = get_kopi_colors(cx);
        let has_selected = self.app_state.selected_entry_id.is_some();
        let is_copied = self.show_copied_tooltip;
        let is_hovered = self
            .hovered_icons
            .contains(&SharedString::from("copy-button"));

        if has_selected {
            if is_copied {
                div()
                    .id("copy-button")
                    .absolute()
                    .top(HEADER_CLEARANCE_HEIGHT)
                    .right(COPY_BUTTON_OFFSET)
                    .child(
                        div()
                            .id("copy-icon-container")
                            .size(COPY_BUTTON_SIZE)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                svg()
                                    .path(icons::ICON_CHECK)
                                    .text_color(colors.foreground)
                                    .with_animation(
                                        "check-icon-anim",
                                        Animation::new(Duration::from_millis(300))
                                            .with_easing(ease_in_out),
                                        move |icon, delta| {
                                            let scale = if delta < 0.5 {
                                                COPY_ANIMATION_SCALE_MIN
                                                    + (COPY_ANIMATION_SCALE_DELTA * delta)
                                            } else {
                                                1.0
                                            };
                                            icon.size(COPY_BUTTON_SIZE * scale)
                                        },
                                    ),
                            ),
                    )
                    .into_any_element()
            } else {
                div()
                    .id("copy-button")
                    .absolute()
                    .top(HEADER_CLEARANCE_HEIGHT)
                    .right(COPY_BUTTON_OFFSET)
                    .cursor(CursorStyle::PointingHand)
                    .on_hover(cx.listener(|this, hovered: &bool, _window, cx| {
                        if *hovered {
                            this.hovered_icons.insert(SharedString::from("copy-button"));
                        } else {
                            this.hovered_icons
                                .remove(&SharedString::from("copy-button"));
                        }
                        cx.notify();
                    }))
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            let content = this.editor.state.read(cx).text().to_string();
                            if !content.is_empty() {
                                this.copy_to_clipboard(content, window, cx);
                            }
                        }),
                    )
                    .child(
                        div()
                            .id("copy-icon-container")
                            .size(COPY_BUTTON_SIZE)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(animated_icon_svg(
                                icons::ICON_COPY,
                                COPY_BUTTON_SIZE,
                                colors.foreground_muted,
                                colors.foreground,
                                is_hovered,
                                0.03,
                                "copy-icon-hover-anim".to_string(),
                            )),
                    )
                    .into_any_element()
            }
        } else {
            div().into_any_element()
        }
    }

    fn render_sidebar_toggle(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = get_kopi_colors(cx);
        let is_hovered = self
            .hovered_icons
            .contains(&SharedString::from("sidebar-toggle"));

        div()
            .id("sidebar-toggle")
            .flex()
            .items_center()
            .justify_center()
            .cursor(CursorStyle::PointingHand)
            .on_hover(cx.listener(|this, hovered: &bool, _window, cx| {
                if *hovered {
                    this.hovered_icons
                        .insert(SharedString::from("sidebar-toggle"));
                } else {
                    this.hovered_icons
                        .remove(&SharedString::from("sidebar-toggle"));
                }
                cx.notify();
            }))
            .child(animated_icon_svg(
                icons::ICON_SIDEBAR_TOGGLE,
                IconSize::XLarge.pixels(),
                colors.foreground_muted,
                colors.foreground,
                is_hovered,
                0.03,
                "sidebar-toggle-icon-anim".to_string(),
            ))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, _window, cx| {
                    this.hovered_icons
                        .remove(&SharedString::from("sidebar-toggle"));
                    this.app_state.toggle_sidebar();
                    cx.notify();
                }),
            )
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = get_kopi_colors(cx);

        if !self.app_state.sidebar_visible {
            return div()
                .flex_shrink_0()
                .w(SIDEBAR_SECTION_HEIGHT + SPACING_LG)
                .h_full()
                .bg(colors.sidebar_background)
                .border_r(BORDER_WIDTH)
                .border_color(colors.border)
                .flex()
                .flex_col()
                .items_center()
                .pt(SPACING_SM)
                .child(self.render_sidebar_toggle(cx))
                .into_any_element();
        }

        let (starred, recents) = self.app_state.partitioned_entries();
        let has_starred = !starred.is_empty();
        let has_recents = !recents.is_empty();

        div()
            .sidebar_container(&colors)
            .flex_shrink_0()
            .w(gpui::px(self.app_state.sidebar_width))
            .flex()
            .flex_col()
            .overflow_hidden()
            .child({
                let capture_on = self.clipboard_monitor.capture_editor_copies();
                div()
                    .h(HEADER_CLEARANCE_HEIGHT + SPACING_XS)
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_between()
                    .pl(SPACING_MD)
                    .pr(SPACING_SM)
                    .child(if capture_on {
                        div()
                            .px(SPACING_XS)
                            .py(gpui::px(1.))
                            .rounded(BORDER_RADIUS_SM)
                            .bg(colors.accent)
                            .text_color(colors.background)
                            .text_size(FONT_SIZE_XS)
                            .child("CAPTURE")
                            .into_any_element()
                    } else {
                        div().into_any_element()
                    })
                    .child(self.render_sidebar_toggle(cx))
            })
            .child(self.render_search_section(cx))
            .child(
                div().flex_1().min_h_0().child(
                    div()
                        .scrollable(gpui::Axis::Vertical)
                        .id("sidebar-scroll")
                        .children(if has_starred {
                            vec![
                                self.render_sidebar_section("Starred", starred, cx)
                                    .into_any_element(),
                            ]
                        } else {
                            vec![]
                        })
                        .children(if has_recents {
                            vec![
                                self.render_sidebar_section("Recents", recents, cx)
                                    .into_any_element(),
                            ]
                        } else {
                            vec![]
                        })
                        .children(if !has_starred && !has_recents {
                            let message = if self.app_state.is_searching() {
                                "No results found"
                            } else {
                                "No clipboard items yet"
                            };
                            vec![
                                div()
                                    .flex_1()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(FONT_SIZE_SM)
                                    .text_color(colors.foreground_muted)
                                    .child(message)
                                    .into_any_element(),
                            ]
                        } else {
                            vec![]
                        })
                        .when(self.app_state.has_more_entries, |scrollable| {
                            let hover_color = colors.hover;
                            scrollable.child(
                                gpui::div()
                                    .id("load-more")
                                    .py(SPACING_MD)
                                    .flex()
                                    .justify_center()
                                    .child(
                                        gpui::div()
                                            .px(SPACING_MD)
                                            .py(SPACING_XS)
                                            .rounded(BORDER_RADIUS_SM)
                                            .bg(colors.background_secondary)
                                            .text_size(FONT_SIZE_SM)
                                            .text_color(colors.foreground_muted)
                                            .hover(move |s| s.bg(hover_color))
                                            .child(if self.app_state.loading_more {
                                                "Loading..."
                                            } else {
                                                "Load more"
                                            })
                                            .on_mouse_down(
                                                gpui::MouseButton::Left,
                                                cx.listener(|this, _, _window, cx| {
                                                    this.app_state.load_more_entries();
                                                    this.clear_all_entry_hover_states();
                                                    cx.notify();
                                                }),
                                            ),
                                    ),
                            )
                        }),
                ),
            )
            .into_any_element()
    }

    fn render_search_section(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = get_kopi_colors(cx);

        div()
            .id("search-section")
            .mx(SPACING_XS)
            .mt(SPACING_XS)
            .mb(SPACING_SM)
            .h(SIDEBAR_SECTION_HEIGHT)
            .flex()
            .items_center()
            .gap(SPACING_XS)
            .px(SPACING_SM)
            .rounded(BORDER_RADIUS_SM)
            .bg(colors.hover)
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.skip_blur = true;
                    this.search_input_state.update(cx, |state, cx| {
                        state.focus(window, cx);
                    });
                }),
            )
            .child(
                svg()
                    .path(icons::ICON_SEARCH)
                    .size(IconSize::Small.pixels())
                    .text_color(colors.foreground)
                    .flex_shrink_0(),
            )
            .child(
                div()
                    .flex_1()
                    .overflow_hidden()
                    .text_size(FONT_SIZE_SM)
                    .child(
                        Input::new(&self.search_input_state)
                            .appearance(false)
                            .small()
                            .w_full(),
                    ),
            )
    }

    fn render_sidebar_section(
        &self,
        title: &str,
        entries: Vec<&EntryMetadata>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = get_kopi_colors(cx);
        let title_owned = title.to_string();

        div()
            .flex()
            .flex_col()
            .mt(SPACING_SM)
            .child(
                div()
                    .sidebar_section_header(&colors)
                    .flex()
                    .items_center()
                    .child(title_owned),
            )
            .child(
                div().py(SPACING_XS).children(
                    entries
                        .into_iter()
                        .map(|entry| self.render_sidebar_item(entry, cx).into_any_element())
                        .collect::<Vec<_>>(),
                ),
            )
    }

    fn render_sidebar_item(
        &self,
        entry: &EntryMetadata,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = get_kopi_colors(cx);
        let is_selected = self.app_state.selected_entry_id == Some(entry.id);
        let is_multi_selected = self.app_state.is_multi_selected(entry.id);
        let show_highlight = is_selected || is_multi_selected;
        let entry_id = entry.id;
        let row_group: SharedString = format!("row-{}", entry.id).into();
        let is_pending_delete = self.app_state.is_pending_delete(entry_id);

        if is_pending_delete {
            return div()
                .id(gpui::ElementId::Name(entry.id.to_string().into()))
                .sidebar_item(&colors, false)
                .flex()
                .items_center()
                .justify_between()
                .child({
                    let anim_id: SharedString = format!("fade-out-{}", entry_id).into();
                    div()
                        .id(anim_id.clone())
                        .flex_1()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(entry.title.clone())
                        .with_animation(
                            anim_id,
                            Animation::new(Duration::from_secs(5)),
                            |el, delta| el.opacity(1.0 - delta),
                        )
                })
                .child(self.render_undo_button(entry_id, &colors, cx))
                .into_any_element();
        }

        div()
            .id(gpui::ElementId::Name(entry.id.to_string().into()))
            .group(row_group.clone())
            .sidebar_item(&colors, show_highlight)
            .flex()
            .items_center()
            .justify_between()
            .hover(|s| {
                if !show_highlight {
                    s.bg(colors.hover)
                } else {
                    s
                }
            })
            .child(self.render_sidebar_item_title(entry, &colors, cx))
            .child(self.render_pencil_button(entry_id, &colors, row_group.clone(), cx))
            .child(self.render_star_button(entry, &colors, row_group.clone(), cx))
            .child(self.render_trash_button(entry_id, &colors, row_group.clone(), cx))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(move |this, event: &gpui::MouseDownEvent, window, cx| {
                    if this.title_edit.editing_entry_id == Some(entry_id) {
                        return;
                    }
                    if this.title_edit.editing_entry_id.is_some() {
                        this.save_title_edit(window, cx);
                    }
                    if event.modifiers.shift && this.app_state.selection_anchor_id.is_some() {
                        this.app_state.select_range(entry_id);
                    } else {
                        this.app_state.clear_multi_select();
                        this.app_state.select_entry(entry_id);
                        this.app_state.selection_anchor_id = Some(entry_id);
                        this.sync_editor_to_selection(window, cx);
                    }
                }),
            )
            .into_any_element()
    }

    fn render_sidebar_item_title(
        &self,
        entry: &EntryMetadata,
        colors: &KopiColors,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let is_editing_title = self.title_edit.editing_entry_id == Some(entry.id);

        if is_editing_title {
            div()
                .id("title-edit-container")
                .flex_1()
                .ml(-SPACING_SM)
                .rounded(BORDER_RADIUS_SM)
                .bg(colors.hover)
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(|this, _, _window, _cx| {
                        // Reset grace period to prevent blur from closing the edit
                        this.title_edit.started_at = Some(Instant::now());
                    }),
                )
                .child(
                    Input::new(&self.title_edit.input_state)
                        .appearance(false)
                        .small()
                        .w_full(),
                )
                .into_any_element()
        } else {
            div()
                .flex_1()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(entry.title.clone())
                .into_any_element()
        }
    }

    fn sidebar_button_base(
        &self,
        kind: SidebarButtonKind,
        entry_id: Uuid,
        row_group: SharedString,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<Div> {
        let button_id = kind.id_for(entry_id);
        let kopi_window = cx.entity().clone();

        div()
            .id(button_id.clone())
            .w(SPACING_XL)
            .h(SIDEBAR_ITEM_HEIGHT)
            .rounded(BORDER_RADIUS_SM)
            .flex()
            .items_center()
            .justify_center()
            .cursor(CursorStyle::PointingHand)
            .opacity(0.)
            .group_hover(row_group, |s| s.opacity(1.))
            .on_hover(move |hovered: &bool, _window, cx| {
                kopi_window.update(cx, |this, cx| {
                    if *hovered {
                        this.hovered_icons.insert(button_id.clone());
                    } else {
                        this.hovered_icons.remove(&button_id);
                    }
                    cx.notify();
                });
            })
    }

    fn sidebar_button_icon(
        &self,
        kind: SidebarButtonKind,
        entry_id: Uuid,
        icon_path: &'static str,
        base_color: Hsla,
        hover_color: Hsla,
    ) -> AnyElement {
        let is_hovered = self.hovered_icons.contains(&kind.id_for(entry_id));
        animated_icon_svg(
            icon_path,
            IconSize::Medium.pixels(),
            base_color,
            hover_color,
            is_hovered,
            0.05,
            kind.anim_id_for(entry_id),
        )
        .into_any_element()
    }

    fn render_pencil_button(
        &self,
        entry_id: Uuid,
        colors: &KopiColors,
        row_group: SharedString,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let is_editing_title = self.title_edit.editing_entry_id == Some(entry_id);
        let kopi_window = cx.entity().clone();

        if is_editing_title {
            return div()
                .w(SPACING_XL)
                .h(SIDEBAR_ITEM_HEIGHT)
                .invisible()
                .into_any_element();
        }

        self.sidebar_button_base(SidebarButtonKind::Pencil, entry_id, row_group, cx)
            .on_mouse_down(gpui::MouseButton::Left, move |_, window, cx| {
                kopi_window.update(cx, |this, cx| {
                    this.start_title_edit(entry_id, window, cx);
                });
            })
            .child(self.sidebar_button_icon(
                SidebarButtonKind::Pencil,
                entry_id,
                icons::ICON_PENCIL,
                colors.foreground_muted,
                colors.foreground,
            ))
            .into_any_element()
    }

    fn render_star_button(
        &self,
        entry: &EntryMetadata,
        colors: &KopiColors,
        row_group: SharedString,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let entry_id = entry.id;
        let is_starred = entry.is_starred;
        let kopi_window = cx.entity().clone();

        let is_hovered = self
            .hovered_icons
            .contains(&SidebarButtonKind::Star.id_for(entry_id));

        let (icon_path, base_color) = match (is_starred, is_hovered) {
            (true, false) => (icons::ICON_STAR_FILLED, colors.foreground),
            (true, true) => (icons::ICON_STAR, colors.foreground_muted),
            (false, false) => (icons::ICON_STAR, colors.foreground_muted),
            (false, true) => (icons::ICON_STAR_FILLED, colors.foreground),
        };

        self.sidebar_button_base(SidebarButtonKind::Star, entry_id, row_group, cx)
            .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
                kopi_window.update(cx, |this, cx| {
                    this.app_state.toggle_starred(entry_id);
                    cx.notify();
                });
            })
            .child(self.sidebar_button_icon(
                SidebarButtonKind::Star,
                entry_id,
                icon_path,
                base_color,
                colors.foreground,
            ))
    }

    fn render_trash_button(
        &self,
        entry_id: Uuid,
        colors: &KopiColors,
        row_group: SharedString,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let kopi_window = cx.entity().clone();

        self.sidebar_button_base(SidebarButtonKind::Trash, entry_id, row_group, cx)
            .mr(-SPACING_MD)
            .on_mouse_down(gpui::MouseButton::Left, move |_, window, cx| {
                kopi_window.update(cx, |this, cx| {
                    this.app_state.clear_multi_select();
                    this.app_state.soft_delete(entry_id);
                    this.clear_entry_hover_state(entry_id);
                    cx.notify();
                });
                let entity = kopi_window.clone();
                window
                    .spawn(cx, async move |cx| {
                        Timer::after(std::time::Duration::from_secs(5)).await;
                        if let Err(e) = entity.update(cx, |this: &mut KopiWindow, cx| {
                            if this.app_state.is_pending_delete(entry_id) {
                                this.app_state.clear_pending_delete(entry_id);
                                cx.notify();
                            }
                        }) {
                            warn!("Failed to clear pending delete: {:?}", e);
                        }
                    })
                    .detach();
            })
            .child(self.sidebar_button_icon(
                SidebarButtonKind::Trash,
                entry_id,
                icons::ICON_TRASH,
                colors.foreground_muted,
                colors.foreground,
            ))
    }

    fn render_undo_button(
        &self,
        entry_id: Uuid,
        colors: &KopiColors,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let kopi_window = cx.entity().clone();
        let is_hovered = self
            .hovered_icons
            .contains(&SharedString::from(format!("undo-{}", entry_id)));
        let button_id: SharedString = format!("undo-{}", entry_id).into();
        let button_id_clone = button_id.clone();

        div()
            .id(button_id.clone())
            .w(SPACING_XL)
            .h(SIDEBAR_ITEM_HEIGHT)
            .mr(-SPACING_MD)
            .flex()
            .items_center()
            .justify_center()
            .cursor(CursorStyle::PointingHand)
            .opacity(1.)
            .on_hover(move |hovered: &bool, _window, cx| {
                let id = button_id_clone.clone();
                kopi_window.update(cx, |this, cx| {
                    if *hovered {
                        this.hovered_icons.insert(id);
                    } else {
                        this.hovered_icons.remove(&button_id_clone);
                    }
                    cx.notify();
                });
            })
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(move |this, _, _window, cx| {
                    this.app_state.undo_delete(entry_id);
                    cx.notify();
                }),
            )
            .child(animated_icon_svg(
                icons::ICON_UNDO,
                IconSize::Medium.pixels(),
                colors.foreground_muted,
                colors.foreground,
                is_hovered,
                0.05,
                format!("undo-icon-anim-{}", entry_id),
            ))
    }

    fn render_content(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = get_kopi_colors(cx);
        let has_selection = self.app_state.selected_entry_id.is_some();

        div()
            .id("content-container")
            .content_container(&colors)
            .flex()
            .flex_col()
            .overflow_hidden()
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.skip_blur = true;
                    this.editor.state.update(cx, |state, cx| {
                        state.focus(window, cx);
                    });
                }),
            )
            .child(
                div()
                    .flex_1()
                    .overflow_hidden()
                    .pt(SPACING_SM)
                    .pb(SPACING_SM)
                    .pl(SPACING_SM)
                    .children(if has_selection {
                        vec![
                            Input::new(&self.editor.state)
                                .appearance(false)
                                .bordered(false)
                                .h_full()
                                .into_any_element(),
                        ]
                    } else {
                        vec![
                            div()
                                .flex_1()
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(FONT_SIZE_BASE)
                                .text_color(colors.foreground_muted)
                                .child("Copy something to get started")
                                .into_any_element(),
                        ]
                    }),
            )
    }

    fn copy_to_clipboard(&mut self, content: String, window: &mut Window, cx: &mut Context<Self>) {
        match self.clipboard_monitor.copy_to_clipboard(&content) {
            Ok(()) => {
                self.show_copied_tooltip = true;

                let entity = cx.entity().clone();
                window
                    .spawn(cx, async move |cx| {
                        Timer::after(std::time::Duration::from_millis(750)).await;
                        if let Err(e) = entity.update(cx, |this: &mut KopiWindow, cx| {
                            this.show_copied_tooltip = false;
                            cx.notify();
                        }) {
                            warn!("Failed to hide copied tooltip: {:?}", e);
                        }
                    })
                    .detach();
            }
            Err(e) => {
                error!("Failed to copy to clipboard: {}", e);
            }
        }
    }
}

impl Render for KopiWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = get_kopi_colors(cx);
        let theme = cx.theme();

        div()
            .id("kopi-root")
            .track_focus(&self.focus_handle)
            .font_family(theme.font_family.clone())
            .text_size(theme.font_size)
            .size_full()
            .flex()
            .flex_col()
            .bg(colors.background)
            .text_color(colors.foreground)
            .relative()
            .on_action(cx.listener(Self::handle_cancel_title_edit))
            .on_action(cx.listener(Self::handle_delete_selected))
            .on_action(cx.listener(Self::handle_toggle_capture))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, window, _cx| {
                    if this.skip_blur {
                        this.skip_blur = false;
                        return;
                    }
                    if this.is_within_title_edit_grace_period() {
                        return;
                    }
                    this.focus_handle.focus(window);
                }),
            )
            .child(self.render_header(cx))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_row()
                    .overflow_hidden()
                    .child(self.render_sidebar(cx))
                    .child(self.render_content(cx)),
            )
            .child(self.render_copy_button(cx))
    }
}
