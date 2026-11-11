//! The real first run's replay strip, in test and debug builds only
//! (`hex_dev_onboarding`): restart it, go back one beat, and play another
//! joke.

use rune_kit::{Button, ButtonVariant, ControlSize};

use super::super::live;
use super::*;

/// Whether this build has the strip, and the jokes that swear.
const ON: bool = cfg!(hex_dev_onboarding);

impl Onboard {
    /// The first run from its opener, the conversation opening.
    pub(crate) fn restart(&mut self) {
        self.opener();
    }

    /// The start of the beat before this one.
    pub(crate) fn back_one_beat(&mut self) {
        match self.step {
            Step::YourTurn | Step::Hearing | Step::Heard => self.opener(),
            Step::Microphone
            | Step::Accessibility
            | Step::Try
            | Step::TryHearing
            | Step::TryHeard => self.with_hex(),
            Step::Ready if self.has_key => self.jump_to_try(),
            Step::Ready => self.with_hex(),
            Step::Key | Step::Parked | Step::Done => self.opener(),
        }
    }

    /// The next bundled joke, from the opener.
    pub(crate) fn next_joke(&mut self) {
        Script::choose_next();
        self.scene = typing::Scene::build(&self.timing);
        self.take = Script::chosen().take();
        self.opener();
    }

    /// `"Without Hex"` from the top: the conversation opens as it would
    /// after the key step.
    fn opener(&mut self) {
        self.jump_to_demo();
        self.auto_due_at = Some(self.now);
    }

    /// `"With Hex"` from the top, `"Without Hex"` just behind it.
    fn with_hex(&mut self) {
        self.jump_to_demo();
        let now = self.now;
        self.typing_from = Some(now - self.scene.duration() - COMEDY_HOLD);
        self.step = Step::YourTurn;
        self.voice_from = Some(now);
        self.hud_from = Some(now);
        self.sfx.on_rewind(now);
        self.auto_due_at = Some(now + versus::BEFORE_KEY);
    }
}

/// The strip in the window's corner, on the real first run.
pub(crate) fn strip(p: Paint, s: &Onboard) -> Option<El> {
    if !ON || s.live.is_none() {
        return None;
    }
    let button = |key: &str, label: String, f: fn(&mut Onboard)| {
        Button::new(label)
            .variant(ButtonVariant::GhostMuted)
            .control_size(ControlSize::Small)
            .on_press(live::on_ui(f))
            .build(p.k)
            .key(key.to_string())
    };
    Some(
        row()
            .key("replay")
            .pin(Some(12.0), None, None, Some(12.0))
            .gap(2.0)
            .center_y()
            .child(button("restart", "Restart".into(), Onboard::restart))
            .child(button("back", "Back".into(), Onboard::back_one_beat))
            .child(button(
                "joke",
                format!("Joke: {}", Script::chosen_name()),
                Onboard::next_joke,
            )),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn back_from_with_hex_opens_without_hex_from_the_top() {
        let mut s = Onboard::new(Flow::FirstRun);
        s.now = 5.0;
        s.with_hex();
        assert_eq!(s.step, Step::YourTurn);
        s.back_one_beat();
        assert_eq!(s.step, Step::Parked);
        assert_eq!(s.typing_from, None);
        assert_eq!(s.auto_due_at, Some(5.0));
    }
}
