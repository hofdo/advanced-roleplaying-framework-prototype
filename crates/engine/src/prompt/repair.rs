/// Build a repair prompt that asks the LLM to return corrected JSON for the
/// `WorldStateDelta` schema, given the malformed output it previously returned.
///
/// The repair call goes directly to `provider.generate()`; it must NOT go
/// through the full turn pipeline.
pub fn repair_prompt(raw_output: &str) -> String {
    format!(
        "The following output was malformed JSON. \
         Return only valid JSON matching the WorldStateDelta schema. \
         Do not include any explanation or markdown.\n\
         \n\
         Schema fields: facts_to_add, npc_changes, faction_changes, \
         quest_changes, clock_changes, relationship_changes, \
         location_change, event_log_entries\n\
         \n\
         Malformed output:\n{raw_output}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repair_prompt_contains_schema_fields_and_raw_output() {
        let prompt = repair_prompt("{ bad json }");
        assert!(prompt.contains("facts_to_add"));
        assert!(prompt.contains("npc_changes"));
        assert!(prompt.contains("{ bad json }"));
    }
}
