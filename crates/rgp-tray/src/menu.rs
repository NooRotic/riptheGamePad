use rgp_core::ProfileId;

pub fn next_profile(current: &ProfileId, all: &[ProfileId]) -> ProfileId {
    if all.is_empty() {
        return current.clone();
    }
    let idx = all.iter().position(|p| p == current).unwrap_or(0);
    all[(idx + 1) % all.len()].clone()
}

pub fn prev_profile(current: &ProfileId, all: &[ProfileId]) -> ProfileId {
    if all.is_empty() {
        return current.clone();
    }
    let idx = all.iter().position(|p| p == current).unwrap_or(0);
    all[(idx + all.len() - 1) % all.len()].clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pids(names: &[&str]) -> Vec<ProfileId> {
        names.iter().map(|n| ProfileId(n.to_string())).collect()
    }

    #[test]
    fn next_profile_wraps_around() {
        let all = pids(&["a", "b", "c"]);
        assert_eq!(next_profile(&"c".into(), &all).0, "a");
        assert_eq!(next_profile(&"a".into(), &all).0, "b");
        assert_eq!(next_profile(&"b".into(), &all).0, "c");
    }

    #[test]
    fn prev_profile_wraps_around() {
        let all = pids(&["a", "b", "c"]);
        assert_eq!(prev_profile(&"a".into(), &all).0, "c");
        assert_eq!(prev_profile(&"b".into(), &all).0, "a");
        assert_eq!(prev_profile(&"c".into(), &all).0, "b");
    }

    #[test]
    fn current_not_in_list_starts_from_zero() {
        let all = pids(&["a", "b", "c"]);
        let result = next_profile(&"unknown".into(), &all);
        assert_eq!(result.0, "b"); // unknown maps to idx 0, so next is idx 1
    }

    #[test]
    fn empty_list_returns_current() {
        let all: Vec<ProfileId> = vec![];
        let result = next_profile(&"x".into(), &all);
        assert_eq!(result.0, "x");
    }
}
