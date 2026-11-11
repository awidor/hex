//! Pieces for the next onboarding, and the options built from them as
//! playground stories (only with Labs; the shipping
//! tour in [`onboarding`](super::onboarding) doesn't use them yet).
//!
//! - [`gpu_stage`] & [`hud_stage`]: flat Rune UI + Hex's real `Dot scope`
//!   phosphor dot-matrix HUD (`IndicatorModel` + `rune_hud::Painter` on
//!   `rune_hud.wgsl`).
//! - [`hex_meter`]: the thin single-hexagon mark (`t573` Hairline) and the
//!   logo `"Hex is ready"` builds.
//! - [`cards`]: example macOS notifications to answer, drawn on theme tokens.
//! - [`field_text`]: a reply laid out by hand, for a caret mid-line and
//!   the slots words fly into.
//! - [`flight`] & [`sparks`]: word flight and crisp landing rings/sparks.
//! - [`keyboard`]: 14.5u constant-pitch 2D Apple Magic Keyboard with slow
//!   arrival and smooth transitions, and [`typing`], the CEO typo script.
//! - [`permissions`]: calm permission grants and the escaping Accessibility
//!   helper panel that detaches from Hex's card and parks under System Settings.
//! - [`sfx`]: four auditionable typing sound candidates (`Taptic tick`,
//!   `Muted felt`, `Soft warm`, `Silent typing`), shortcut keycap clicks,
//!   motion cues, and Hex's `#292` `feedback` tones.
//! - [`mic_line`]: the microphone's level and selector capsule.
//! - [`landing`]: shared typography, zero-bounce transitions, and `"Hex is ready"`.

mod cards;
mod composition;
mod dictated;
mod field_text;
mod fireworks;
mod first_run;
mod first_screen;
mod fixture;
mod flight;
mod flows;
pub(crate) mod game;
pub(crate) mod gpu_stage;
mod hex_meter;
mod high_score;
mod hud_stage;
mod inbox;
mod keyboard;
mod landing;
pub(super) mod live;
mod login;
mod mic_line;
mod permissions;
mod scene_script;
pub(super) mod scenes;
mod sfx;
mod sparks;
mod stories;
mod triumph;
mod typing;
mod versus;

pub(crate) use game::add as game;
pub(super) use stories::Onboard;
pub(crate) use stories::{add as stories, set_live_window};
