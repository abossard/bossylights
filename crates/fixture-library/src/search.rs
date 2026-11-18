//! Search by maker and model: every word of the query must be near a word of the
//! fixture's name, ignoring case. Near is a prefix, or a few typos (by word length).

use crate::model::Fixture;
use alloc::string::String;
use alloc::vec::Vec;

/// Typos allowed in a word of this length: none under 3 letters, one up to 5, then two.
fn allowed_typos(len: usize) -> usize {
    match len {
        0..=2 => 0,
        3..=5 => 1,
        _ => 2,
    }
}

fn words(text: &str) -> Vec<String> {
    text.split(|c: char| c.is_whitespace() || c == '-' || c == '_' || c == '/')
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// Edit distance between two words: insert, delete, change one letter.
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let first: Vec<usize> = (0..=b.len()).collect();
    let last = a.chars().enumerate().fold(first, |previous, (i, ca)| {
        let mut row = Vec::with_capacity(previous.len());
        row.push(i + 1);
        for (j, cb) in b.iter().enumerate() {
            let change = previous.get(j).copied().unwrap_or(0) + usize::from(ca != *cb);
            let delete = previous.get(j + 1).copied().unwrap_or(0) + 1;
            let insert = row.last().copied().unwrap_or(0) + 1;
            row.push(change.min(delete).min(insert));
        }
        row
    });
    last.last().copied().unwrap_or(0)
}

/// How far a query word is from the nearest of the name's words: the letters it leaves
/// out when it starts the word, else the typos. `None` when too far.
fn nearest(query: &str, name: &[String]) -> Option<usize> {
    let length = query.chars().count();
    let budget = allowed_typos(length);
    name.iter()
        .filter_map(|word| {
            if word.starts_with(query) {
                Some(word.chars().count() - length)
            } else {
                Some(distance(query, word)).filter(|d| *d <= budget)
            }
        })
        .min()
}

/// The fixtures that match every word of `query`, best first, then by maker and model.
/// An empty query matches nothing.
pub fn search<'a>(library: &'a [Fixture], query: &str) -> Vec<&'a Fixture> {
    let query = words(query);
    if query.is_empty() {
        return Vec::new();
    }
    let mut found: Vec<(usize, &Fixture)> = library
        .iter()
        .filter_map(|fixture| {
            let name = words(&alloc::format!("{} {}", fixture.maker, fixture.model));
            let total = query
                .iter()
                .map(|word| nearest(word, &name))
                .sum::<Option<usize>>()?;
            Some((total, fixture))
        })
        .collect();
    found.sort_by(|(a, fa), (b, fb)| {
        a.cmp(b)
            .then_with(|| (&fa.maker, &fa.model).cmp(&(&fb.maker, &fb.model)))
    });
    found.into_iter().map(|(_, fixture)| fixture).collect()
}
