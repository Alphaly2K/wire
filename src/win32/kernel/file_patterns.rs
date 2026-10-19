//! Shared guest filename wildcard algorithm.
pub(crate) fn wildcard(pattern: &str, name: &str) -> bool {
    let pattern = if pattern == "*.*" { "*" } else { pattern };
    let p: Vec<_> = pattern.to_lowercase().chars().collect();
    let n: Vec<_> = name.to_lowercase().chars().collect();
    let mut dp = vec![false; n.len() + 1];
    dp[0] = true;
    for c in p {
        let mut next = vec![false; n.len() + 1];
        if c == '*' {
            next[0] = dp[0];
        }
        for i in 1..=n.len() {
            next[i] = if c == '*' {
                dp[i] || next[i - 1]
            } else {
                dp[i - 1] && (c == '?' || c == n[i - 1])
            };
        }
        dp = next;
    }
    dp[n.len()]
}
