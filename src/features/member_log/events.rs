/// アカウント作成からの経過日数がしきい値未満なら新規アカウントとみなす。
pub fn is_new_account(created_at_unix: i64, now_unix: i64, warn_days: i64) -> bool {
    let age_days = (now_unix - created_at_unix) / 86_400;
    age_days < warn_days
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_younger_than_threshold_is_new() {
        let now = 1_000_000_000;
        let created_at = now - 3 * 86_400;
        assert!(is_new_account(created_at, now, 7));
    }

    #[test]
    fn account_older_than_threshold_is_not_new() {
        let now = 1_000_000_000;
        let created_at = now - 30 * 86_400;
        assert!(!is_new_account(created_at, now, 7));
    }

    #[test]
    fn account_exactly_at_threshold_is_not_new() {
        let now = 1_000_000_000;
        let created_at = now - 7 * 86_400;
        assert!(!is_new_account(created_at, now, 7));
    }
}
