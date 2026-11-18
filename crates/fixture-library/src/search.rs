//! Search by maker and model: every word of the query must be near a word of the
//! fixture's name, ignoring case. Near is a prefix, or a few typos (by word length).
//! A prefix always ranks above a typo. Words end at every character that is not a letter or a digit.

use crate::model::Fixture;
use alloc::vec::Vec;

/// Typos allowed in a word of this length: none under 3 letters, one up to 5, then two.
fn allowed_typos(len: usize) -> usize {
    match len {
        0..=2 => 0,
        3..=5 => 1,
        _ => 2,
    }
}

/// The words of a text: runs of letters and digits.
fn words(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
}

fn letters(word: &str) -> impl Iterator<Item = char> + '_ {
    word.chars().flat_map(char::to_lowercase)
}

/// Edit distance between two words: insert, delete, change one letter. `row` is scratch space.
fn distance(a: &[char], b: &str, row: &mut Vec<usize>) -> usize {
    row.clear();
    row.extend(0..=a.len());
    for (i, cb) in letters(b).enumerate() {
        let mut diagonal = row.first().copied().unwrap_or(0);
        if let Some(first) = row.first_mut() {
            *first = i + 1;
        }
        let mut left = i + 1;
        for (cell, ca) in row.iter_mut().skip(1).zip(a) {
            let up = *cell;
            left = (diagonal + usize::from(*ca != cb))
                .min(up + 1)
                .min(left + 1);
            diagonal = up;
            *cell = left;
        }
    }
    row.last().copied().unwrap_or(0)
}

/// A prefix costs the letters it leaves out and sorts before every typo, which costs its distance.
const TYPO_TIER: usize = 1_000;

/// How far a query word is from the nearest of the name's words, `None` when too far.
fn nearest(query: &[char], name: &str, row: &mut Vec<usize>) -> Option<usize> {
    let budget = allowed_typos(query.len());
    words(name)
        .filter_map(|word| {
            let length = letters(word).count();
            if letters(word).zip(query).filter(|(w, q)| w == *q).count() == query.len()
                && length >= query.len()
            {
                Some(length - query.len())
            } else if length.abs_diff(query.len()) <= budget {
                Some(distance(query, word, row))
                    .filter(|d| *d <= budget)
                    .map(|d| TYPO_TIER + d)
            } else {
                None
            }
        })
        .min()
}

/// The fixtures that match every word of `query`, best first, then by maker and model.
/// An empty query matches nothing.
pub fn search<'a>(library: &'a [Fixture], query: &str) -> Vec<&'a Fixture> {
    let query: Vec<Vec<char>> = words(query).map(|word| letters(word).collect()).collect();
    if query.is_empty() {
        return Vec::new();
    }
    let mut row = Vec::new();
    let mut found: Vec<(usize, &Fixture)> = library
        .iter()
        .filter_map(|fixture| {
            let total = query
                .iter()
                .map(|word| {
                    let maker = nearest(word, &fixture.maker, &mut row);
                    let model = nearest(word, &fixture.model, &mut row);
                    maker.into_iter().chain(model).min()
                })
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
