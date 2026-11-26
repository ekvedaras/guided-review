//! The agent picker: opened from the tour's station, it lists herdr's Claude terminals off the UI
//! thread and hands Show and Use to the session.

use gpui::{Context, Window};

use super::OneStepLayout;
use crate::app::components::agent_picker::AgentPickerEvent;

impl OneStepLayout {
    pub(super) fn open_agent_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let session = self.session.read(cx);
        let agents = session.tour_agents();
        let subject = session.review().subject().clone();
        let of_what = if subject.agent_changes_code() {
            "this folder's agent"
        } else {
            "this PR's tour agent"
        };
        self.agent_picker
            .update(cx, |picker, cx| picker.open(of_what.into(), window, cx));
        let picker = self.agent_picker.downgrade();
        cx.spawn(async move |_, cx| {
            let listed = cx
                .background_executor()
                .spawn(async move { agents.agent_choices(&subject) })
                .await
                .map_err(|error| format!("{error:#}"));
            picker
                .update(cx, |picker, cx| picker.listed(listed, cx))
                .ok();
        })
        .detach();
    }

    pub(super) fn agent_picker_asked(&mut self, event: &AgentPickerEvent, cx: &mut Context<Self>) {
        match event {
            AgentPickerEvent::Use(terminal) => {
                let terminal = terminal.clone();
                self.session
                    .update(cx, |session, cx| session.use_tour_agent(terminal, cx));
            }
            AgentPickerEvent::Show(terminal) => {
                let terminal = terminal.clone();
                self.session
                    .update(cx, |session, cx| session.show_terminal(terminal, cx));
            }
        }
    }
}
