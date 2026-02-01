use gpui::{
    Animation, AnimationExt, ElementId, Hsla, IntoElement, Pixels, SharedString, Styled, Svg, svg,
};
use std::time::Duration;

const HOVER_ANIMATION_MS: u64 = 150;

pub fn animated_opacity_svg(
    path: &'static str,
    size: Pixels,
    color: Hsla,
    target_opacity: f32,
    anim_id: String,
) -> impl IntoElement {
    let full_id: ElementId = ElementId::Name(SharedString::from(if target_opacity > 0.5 {
        format!("{}-visible", anim_id)
    } else {
        format!("{}-hidden", anim_id)
    }));
    let start_opacity = 1.0 - target_opacity;
    svg()
        .path(path)
        .text_color(color)
        .absolute()
        .with_animation(
            full_id,
            Animation::new(Duration::from_millis(HOVER_ANIMATION_MS)),
            move |icon: Svg, delta| {
                let opacity = start_opacity + (target_opacity - start_opacity) * delta;
                let scale = if target_opacity > 0.5 {
                    let bounce_delta = if delta < 0.5 {
                        delta * 2.0
                    } else {
                        1.0 - (delta - 0.5) * 2.0
                    };
                    let eased = (bounce_delta * std::f32::consts::PI / 2.0).sin();
                    1.0 - 0.05 * eased
                } else {
                    1.0
                };
                icon.opacity(opacity).size(size * scale)
            },
        )
}

pub fn animated_icon_svg(
    path: &'static str,
    size: Pixels,
    base_color: Hsla,
    hover_color: Hsla,
    is_hovered: bool,
    shrink: f32,
    anim_id: String,
) -> impl IntoElement {
    let full_id: ElementId = ElementId::Name(SharedString::from(if is_hovered {
        format!("{}-hovered", anim_id)
    } else {
        format!("{}-normal", anim_id)
    }));
    svg().path(path).with_animation(
        full_id,
        Animation::new(Duration::from_millis(HOVER_ANIMATION_MS)),
        move |icon: Svg, delta| {
            let color = if is_hovered {
                blend_colors(base_color, hover_color, delta)
            } else {
                blend_colors(hover_color, base_color, delta)
            };
            let scale = if is_hovered {
                let bounce_delta = if delta < 0.5 {
                    delta * 2.0
                } else {
                    1.0 - (delta - 0.5) * 2.0
                };
                let eased = (bounce_delta * std::f32::consts::PI / 2.0).sin();
                1.0 - shrink * eased
            } else {
                1.0
            };
            icon.text_color(color).size(size * scale)
        },
    )
}

fn blend_colors(from: Hsla, to: Hsla, t: f32) -> Hsla {
    // Handle circular hue interpolation - take shortest path
    let mut h_diff = to.h - from.h;
    if h_diff > 0.5 {
        h_diff -= 1.0;
    } else if h_diff < -0.5 {
        h_diff += 1.0;
    }
    let h = (from.h + h_diff * t).rem_euclid(1.0);

    Hsla {
        h,
        s: from.s + (to.s - from.s) * t,
        l: from.l + (to.l - from.l) * t,
        a: from.a + (to.a - from.a) * t,
    }
}
