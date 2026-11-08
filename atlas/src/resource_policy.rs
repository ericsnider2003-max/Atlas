//! Deterministic answers for the resource-conflict questions that should not
//! spend a model turn. The relief engine owns the actual release order; this
//! module exposes that bounded policy to the conversation layer.

/// Answer a question about keeping voice and reasoning usable under memory
/// pressure. `None` means the question is outside this narrow policy seam.
pub fn answer(
    question: &str,
    tier: &str,
    model: &str,
    keep_model_warm: bool,
    worker_ceiling: u32,
    helper_memory_mb: u64,
) -> Option<String> {
    let low = question.to_ascii_lowercase();
    let memory = ["memory", "ram", "pressure", "low space", "short of memory"]
        .iter()
        .any(|word| low.contains(word));
    let voice = low.contains("voice") || low.contains("speech") || low.contains("talk");
    let reasoning = low.contains("reason")
        || low.contains("think")
        || low.contains("model")
        || low.contains("coding");
    if !(memory && voice && reasoning) {
        return None;
    }

    Some(format!(
        "Atlas's bounded response is: protect the active voice or reasoning turn; \
         release idle voice/helper state first, then release an idle model server if pressure persists; \
         never evict an in-use job. The model can reload after pressure clears. \
         Current fit: {tier}, model {model}, model warm {keep_model_warm}, \
         worker ceiling {worker_ceiling}, helpers using about {helper_memory_mb} MB.",
    ))
}

#[cfg(test)]
mod tests {
    use super::answer;

    #[test]
    fn resource_conflict_gets_a_bounded_release_order() {
        let line = answer(
            "What should Atlas do when memory pressure conflicts with voice and reasoning?",
            "voice and reasoning",
            "qwen-local",
            true,
            2,
            640,
        )
        .unwrap();
        assert!(line.contains("protect the active voice or reasoning turn"));
        assert!(line.contains("release idle voice/helper state first"));
        assert!(line.contains("never evict an in-use job"));
        assert!(line.contains("worker ceiling 2"));
    }

    #[test]
    fn unrelated_questions_do_not_get_a_resource_policy_answer() {
        assert!(answer("What is Atlas?", "thinking", "qwen-local", true, 2, 0).is_none());
    }
}
