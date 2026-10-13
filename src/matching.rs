// Fuzzy text matching between Whisper transcripts and book text, and the
// in-order assignment of chapters to candidate breaks.
use std::collections::HashSet;

// Lowercased words with punctuation stripped and numbers spelled out, so
// "Chapter 7." / "CHAPTER VII" / "chapter seven" all become "chapter seven".
pub fn normalize(text: &str) -> Vec<String> {
    let text = text.replace(['\'', '\u{2019}'], "");
    let digit_commas = regex::Regex::new(r"(\d),(\d)").unwrap();
    let text = digit_commas.replace_all(&text, "$1$2").to_lowercase();

    let mut words: Vec<String> = Vec::new();
    for token in text.split(|c: char| !c.is_alphanumeric()).filter(|t| !t.is_empty()) {
        let after_heading = words.last().is_some_and(|w| matches!(w.as_str(), "chapter" | "part" | "book"));
        let number = match token.parse::<u32>() {
            Ok(n) => Some(n),
            Err(_) if after_heading => roman(token),
            Err(_) => None,
        };
        match number {
            Some(n) => words.extend(number_words(n).split(' ').map(String::from)),
            None => words.push(token.to_string()),
        }
    }
    words
}

fn roman(s: &str) -> Option<u32> {
    let value = |c| match c {
        'i' => Some(1), 'v' => Some(5), 'x' => Some(10), 'l' => Some(50),
        'c' => Some(100), 'd' => Some(500), 'm' => Some(1000), _ => None,
    };
    let digits: Vec<u32> = s.chars().map(value).collect::<Option<_>>()?;
    let mut total = 0;
    for (i, &d) in digits.iter().enumerate() {
        if digits.get(i + 1).is_some_and(|&next| next > d) {
            total -= d as i64;
        } else {
            total += d as i64;
        }
    }
    u32::try_from(total).ok().filter(|&n| n > 0)
}

// English words for 0..=9999; larger numbers are left as digits.
pub fn number_words(n: u32) -> String {
    const ONES: [&str; 20] = [
        "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
        "eleven", "twelve", "thirteen", "fourteen", "fifteen", "sixteen", "seventeen", "eighteen", "nineteen",
    ];
    const TENS: [&str; 10] = ["", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety"];

    fn words(n: u32) -> String {
        let (big, unit, rest) = match n {
            0..=19 => return ONES[n as usize].to_string(),
            20..=99 => return match n % 10 {
                0 => TENS[(n / 10) as usize].to_string(),
                r => format!("{} {}", TENS[(n / 10) as usize], ONES[r as usize]),
            },
            100..=999 => (n / 100, "hundred", n % 100),
            _ => (n / 1000, "thousand", n % 1000),
        };
        match rest {
            0 => format!("{} {}", words(big), unit),
            r => format!("{} {} {}", words(big), unit, words(r)),
        }
    }

    if n > 9999 { n.to_string() } else { words(n) }
}

// Fraction of the transcript's word pairs that also appear in the passage.
// Tolerates wrong words and extra narration; unrelated text scores near 0.
pub fn bigram_score(transcript: &[String], passage: &[String]) -> f64 {
    if transcript.len() < 2 {
        return 0.0;
    }
    let passage_pairs: HashSet<(&str, &str)> =
        passage.windows(2).map(|w| (w[0].as_str(), w[1].as_str())).collect();
    let hits = transcript
        .windows(2)
        .filter(|w| passage_pairs.contains(&(w[0].as_str(), w[1].as_str())))
        .count();
    hits as f64 / (transcript.len() - 1) as f64
}

// Assigns chapters (in order) to candidates (in time order), one candidate per
// chapter at most, maximizing the total score. `score(chapter, candidate)` returns
// None when the pair isn't a plausible match. Returns `assigned[chapter] = candidate`.
pub fn align(
    chapters: usize,
    candidates: usize,
    score: impl Fn(usize, usize) -> Option<f64>,
) -> Vec<Option<usize>> {
    // best[k][j]: best total using the first k chapters and first j candidates.
    let mut best = vec![vec![0.0f64; candidates + 1]; chapters + 1];
    for k in 1..=chapters {
        for j in 1..=candidates {
            let take = score(k - 1, j - 1).map_or(f64::MIN, |s| best[k - 1][j - 1] + s);
            best[k][j] = best[k - 1][j].max(best[k][j - 1]).max(take);
        }
    }

    let mut assigned = vec![None; chapters];
    let (mut k, mut j) = (chapters, candidates);
    while k > 0 && j > 0 {
        if best[k][j] == best[k - 1][j] {
            k -= 1;
        } else if best[k][j] == best[k][j - 1] {
            j -= 1;
        } else {
            assigned[k - 1] = Some(j - 1);
            k -= 1;
            j -= 1;
        }
    }
    assigned
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(s: &str) -> Vec<String> {
        normalize(s)
    }

    #[test]
    fn normalize_strips_punctuation_and_case() {
        assert_eq!(words("\u{201C}Don't—stop,\u{201D} she said. Well-known!"), ["dont", "stop", "she", "said", "well", "known"]);
    }

    #[test]
    fn normalize_spells_numbers_and_chapter_numerals() {
        assert_eq!(words("Chapter 7."), ["chapter", "seven"]);
        assert_eq!(words("CHAPTER XXI"), ["chapter", "twenty", "one"]);
        assert_eq!(words("Part IV: 1,200 miles"), ["part", "four", "one", "thousand", "two", "hundred", "miles"]);
        // A lone "I" is a pronoun, not a numeral.
        assert_eq!(words("I said"), ["i", "said"]);
    }

    #[test]
    fn number_words_covers_common_ranges() {
        assert_eq!(number_words(0), "zero");
        assert_eq!(number_words(13), "thirteen");
        assert_eq!(number_words(40), "forty");
        assert_eq!(number_words(105), "one hundred five");
        assert_eq!(number_words(2024), "two thousand twenty four");
        assert_eq!(number_words(12345), "12345");
    }

    const PASSAGE: &str = "It was a bright cold day in April, and the clocks were striking thirteen. \
        Winston Smith, his chin nuzzled into his breast in an effort to escape the vile wind, \
        slipped quickly through the glass doors of Victory Mansions, though not quickly enough \
        to prevent a swirl of gritty dust from entering along with him.";

    #[test]
    fn bigram_score_exact_and_fuzzy() {
        let passage = words(PASSAGE);
        let exact = words("the clocks were striking thirteen Winston Smith his chin nuzzled into his breast");
        assert_eq!(bigram_score(&exact, &passage), 1.0);

        // A few mis-transcribed words and an extra narrator heading.
        let fuzzy = words("Chapter one. It was a bright cold day in April and the clocks were striking 13. Winston Smith his chin muzzled into his breast in an effort to escape the vial wind");
        let s = bigram_score(&fuzzy, &passage);
        assert!(s > 0.6, "fuzzy score {}", s);

        let unrelated = words("Call me Ishmael. Some years ago, never mind how long precisely, having little or no money in my purse");
        let s = bigram_score(&unrelated, &passage);
        assert!(s < 0.15, "unrelated score {}", s);

        assert_eq!(bigram_score(&words("hello"), &passage), 0.0);
        assert_eq!(bigram_score(&[], &passage), 0.0);
    }

    // Score table helper: rows are chapters, columns candidates.
    fn table(rows: &[&[f64]]) -> impl Fn(usize, usize) -> Option<f64> {
        let rows: Vec<Vec<f64>> = rows.iter().map(|r| r.to_vec()).collect();
        move |ch, c| Some(rows[ch][c]).filter(|&s| s > 0.0)
    }

    #[test]
    fn align_picks_best_in_order() {
        // Chapter 0 = front matter (no match), chapters 1-3 match candidates 1, 2, 4.
        let assigned = align(4, 5, table(&[
            &[0.0, 0.0, 0.0, 0.0, 0.0],
            &[0.0, 0.9, 0.0, 0.0, 0.0],
            &[0.0, 0.0, 0.8, 0.3, 0.0],
            &[0.0, 0.0, 0.0, 0.0, 0.7],
        ]));
        assert_eq!(assigned, vec![None, Some(1), Some(2), Some(4)]);
    }

    #[test]
    fn align_never_goes_backwards() {
        // Chapter 1 scores best on candidate 0, but chapter 0 must come first.
        let assigned = align(2, 2, table(&[
            &[0.6, 0.0],
            &[0.9, 0.7],
        ]));
        assert_eq!(assigned, vec![Some(0), Some(1)]);
    }

    #[test]
    fn align_reports_missing_chapters() {
        let assigned = align(3, 2, table(&[
            &[0.9, 0.0],
            &[0.0, 0.0],
            &[0.0, 0.8],
        ]));
        assert_eq!(assigned, vec![Some(0), None, Some(1)]);
    }
}
