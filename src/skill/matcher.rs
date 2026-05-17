use super::Skill;

pub struct SkillMatcher;

impl SkillMatcher {
    /// Return skills whose confidence score exceeds 0.6, sorted descending
    pub fn match_skills<'a>(message: &str, available: &'a [Skill]) -> Vec<(&'a Skill, f32)> {
        let msg = message.to_lowercase();
        let words: Vec<&str> = msg.split_whitespace().collect();

        let mut scored: Vec<(&Skill, f32)> = available
            .iter()
            .filter_map(|skill| {
                let score = Self::score(skill, &words, &msg);
                if score > 0.6 { Some((skill, score)) } else { None }
            })
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored
    }

    fn score(skill: &Skill, words: &[&str], message: &str) -> f32 {
        let mut score = 0.0f32;

        let domain_lower = skill.domain.to_lowercase();
        let name_lower = skill.name.to_lowercase();

        if message.contains(&domain_lower) { score += 0.4; }
        if message.contains(&name_lower) { score += 0.5; }

        if !skill.tags.is_empty() {
            let tag_score: f32 = skill.tags.iter().map(|tag| {
                let tag_lower = tag.to_lowercase();
                // Exact substring match in full message
                if message.contains(tag_lower.as_str()) {
                    return 1.0;
                }
                // Bi-directional partial: word starts-with tag or tag starts-with word
                let partial = words.iter().any(|w| {
                    w.starts_with(tag_lower.as_str()) || tag_lower.starts_with(*w)
                });
                if partial { 0.7 } else { 0.0 }
            }).sum::<f32>() / skill.tags.len() as f32;
            score += tag_score * 0.6;
        }

        score.min(1.0)
    }
}
