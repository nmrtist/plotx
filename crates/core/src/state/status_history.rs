use std::collections::VecDeque;
use std::time::SystemTime;

const MESSAGE_LIMIT: usize = 200;

#[derive(Clone, Debug)]
pub struct StatusMessage {
    pub recorded_at: SystemTime,
    pub text: String,
}

/// Session-only messages formerly displayed in the workspace status strip.
#[derive(Default)]
pub struct StatusHistory {
    messages: VecDeque<StatusMessage>,
    last_observed: String,
}

impl StatusHistory {
    pub fn observe(&mut self, status: &str) {
        if self.last_observed == status {
            return;
        }
        self.last_observed = status.to_owned();
        if status.trim().is_empty() {
            return;
        }
        self.messages.push_back(StatusMessage {
            recorded_at: SystemTime::now(),
            text: status.to_owned(),
        });
        if self.messages.len() > MESSAGE_LIMIT {
            self.messages.pop_front();
        }
    }

    pub fn messages(&self) -> impl DoubleEndedIterator<Item = &StatusMessage> + ExactSizeIterator {
        self.messages.iter()
    }

    pub fn clear(&mut self) {
        // Keep the observation watermark so clearing does not reinsert the current message.
        self.messages.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observations_deduplicate_and_clear_without_reappearing() {
        let mut history = StatusHistory::default();
        history.observe("Loaded");
        history.observe("Loaded");
        assert_eq!(history.messages().len(), 1);
        history.clear();
        history.observe("Loaded");
        assert_eq!(history.messages().len(), 0);
        history.observe("Saved");
        history.observe("Loaded");
        assert_eq!(history.messages().len(), 2);
    }

    #[test]
    fn retains_recent_messages_in_order_with_a_bounded_size() {
        let mut history = StatusHistory::default();
        for index in 0..250 {
            history.observe(&format!("Message {index}"));
        }
        assert_eq!(history.messages().len(), MESSAGE_LIMIT);
        assert_eq!(history.messages().next().unwrap().text, "Message 50");
        assert_eq!(history.messages().next_back().unwrap().text, "Message 249");
    }
}
