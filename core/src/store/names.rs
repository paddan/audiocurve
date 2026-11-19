//! Profile name rules: trimming, truncation, uniqueness and natural sort.

use std::cmp::Ordering;

use super::StoreError;

pub const MAX_NAME_LENGTH: usize = 60;

/// The first `n` characters, with surrounding whitespace trimmed.
fn prefix(s: &str, n: usize) -> String {
    s.chars().take(n).collect::<String>().trim().to_string()
}

/// Trims, truncates, and rejects empty or already used names (case-insensitive). `taken` is the
/// other profiles' names.
pub fn clean_name<'a>(raw: &str, mut taken: impl Iterator<Item = &'a str>) -> Result<String, StoreError> {
    let name = raw.trim();
    if name.is_empty() {
        return Err(StoreError::EmptyName);
    }
    let name = prefix(name, MAX_NAME_LENGTH);
    let key = name.to_lowercase();
    if taken.any(|t| t.to_lowercase() == key) {
        return Err(StoreError::NameTaken);
    }
    Ok(name)
}

/// Like `clean_name`, but a taken name gets a " (n)" suffix, the base truncated so the result fits.
pub fn unique_clean_name<'a>(raw: &str, taken: impl Iterator<Item = &'a str>) -> Result<String, StoreError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(StoreError::EmptyName);
    }
    let taken: Vec<String> = taken.map(str::to_lowercase).collect();
    let is_taken = |c: &str| taken.iter().any(|t| *t == c.to_lowercase());
    let base = prefix(trimmed, MAX_NAME_LENGTH);
    if !is_taken(&base) {
        return Ok(base);
    }
    (2..)
        .map(|n| {
            let suffix = format!(" ({n})");
            prefix(&base, MAX_NAME_LENGTH - suffix.chars().count()) + &suffix
        })
        .find(|c| !is_taken(c))
        .ok_or(StoreError::NameTaken)
}

/// Case-insensitive order with digit runs compared as numbers ("Beta 2" before "Beta 10").
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (la, lb) = (a.to_lowercase(), b.to_lowercase());
    let (mut x, mut y) = (la.chars().peekable(), lb.chars().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return a.cmp(b),
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(c), Some(d)) if c.is_ascii_digit() && d.is_ascii_digit() => {
                let take = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut s = String::new();
                    while let Some(&ch) = it.peek().filter(|ch| ch.is_ascii_digit()) {
                        s.push(ch);
                        it.next();
                    }
                    s
                };
                let (m, n) = (take(&mut x), take(&mut y));
                let (m, n) = (m.trim_start_matches('0'), n.trim_start_matches('0'));
                let ord = m.len().cmp(&n.len()).then_with(|| m.cmp(n));
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            (Some(c), Some(d)) => {
                if c != d {
                    return c.cmp(&d);
                }
                x.next();
                y.next();
            }
        }
    }
}
