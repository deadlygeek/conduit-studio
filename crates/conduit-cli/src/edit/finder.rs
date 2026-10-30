//! Small fuzzy matcher for the find overlay: the query's characters must appear in order;
//! consecutive runs, word starts and short candidates score higher.

/// Score of `candidate` for `query` (higher is better), or `None` if it doesn't match.
/// Case-insensitive; spaces in the query are ignored.
pub fn score(query: &str, candidate: &str) -> Option<i32> {
    let candidate: Vec<char> = candidate.to_lowercase().chars().collect();
    let mut score = 0;
    let mut next = 0;
    let mut prev: Option<usize> = None;
    for q in query.to_lowercase().chars().filter(|c| !c.is_whitespace()) {
        let pos = (next..candidate.len()).find(|&i| candidate[i] == q)?;
        score += 1;
        if prev.is_some_and(|p| p + 1 == pos) {
            score += 5;
        }
        if pos == 0 || matches!(candidate[pos - 1], '_' | '-' | ' ' | '.') {
            score += 3;
        }
        prev = Some(pos);
        next = pos + 1;
    }
    Some(score - candidate.len() as i32 / 8)
}

/// Candidates matching `query`, best first (ties keep input order).
pub fn rank<'a, T>(query: &str, candidates: impl IntoIterator<Item = (T, &'a str)>) -> Vec<T> {
    let mut scored: Vec<(i32, usize, T)> = candidates
        .into_iter()
        .enumerate()
        .filter_map(|(i, (item, text))| score(query, text).map(|s| (s, i, item)))
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored.into_iter().map(|(_, _, item)| item).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subsequence_required() {
        assert!(score("wbg", "white_bg").is_some());
        assert!(score("WBG", "white_bg").is_some());
        assert!(score("gbw", "white_bg").is_none());
        assert_eq!(score("", "anything"), Some(-1));
    }

    #[test]
    fn ranking_prefers_word_starts_and_runs() {
        let names = [
            "black_sq rect",
            "placed position",
            "comp composite",
            "preview preview",
        ];
        let ranked = rank("pl", names.iter().map(|n| (*n, *n)));
        assert_eq!(ranked[0], "placed position");
        let ranked = rank("bsq", names.iter().map(|n| (*n, *n)));
        assert_eq!(ranked, ["black_sq rect"]);
        let ranked = rank("rect", names.iter().map(|n| (*n, *n)));
        assert_eq!(ranked[0], "black_sq rect");
    }
}
