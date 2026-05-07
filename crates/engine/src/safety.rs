pub trait HiddenReasoningStripper: Send + Sync {
    fn strip(&self, text: &str) -> String;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct BasicHiddenReasoningStripper;

impl HiddenReasoningStripper for BasicHiddenReasoningStripper {
    fn strip(&self, text: &str) -> String {
        let mut cleaned = strip_tag_blocks(text, "<think>", "</think>");
        for marker in [
            "Internal reasoning:",
            "Chain of thought:",
            "Hidden reasoning:",
            "GM reasoning:",
        ] {
            if let Some(index) = cleaned.find(marker) {
                cleaned.truncate(index);
            }
        }
        cleaned.trim().to_owned()
    }
}

fn strip_tag_blocks(input: &str, start: &str, end: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut rest = input;

    while let Some(start_index) = rest.find(start) {
        output.push_str(&rest[..start_index]);
        let after_start = &rest[start_index + start.len()..];
        if let Some(end_index) = after_start.find(end) {
            rest = &after_start[end_index + end.len()..];
        } else {
            rest = "";
        }
    }

    output.push_str(rest);
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_think_blocks_and_hidden_reasoning_prefixes() {
        let text = "Visible.<think>secret</think>\nHidden reasoning: should not show";

        assert_eq!(BasicHiddenReasoningStripper.strip(text), "Visible.");
    }
}
