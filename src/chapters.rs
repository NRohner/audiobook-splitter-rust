// Chapter-break detection: silences become candidates, Whisper transcribes the
// speech either side, and the transcripts are matched against the EPUB chapters.
use crate::book::Chapter;
use crate::matching::{align, bigram_score, normalize};
use crate::transcribe::{clip_samples, Transcriber};
use crate::Silence;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ponytail: fixed clip length (spec said 15-30s); make it a setting if tuning needs it.
pub const CLIP_SECONDS: f64 = 20.0;
// Silences this close to a file's start/end are covered by the file boundary itself.
const EDGE_SECONDS: f64 = 1.0;

pub struct AudioFile {
    pub path: PathBuf,
    pub silences: Vec<Silence>,
    pub duration: f64,
}

// A possible chapter break: a silence, or the start of a file (start == end == 0).
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub file: usize,
    pub start: f64,
    pub end: f64,
}

impl Candidate {
    pub fn split_at(&self) -> f64 {
        (self.start + self.end) / 2.0
    }
}

#[derive(Debug, Clone)]
pub struct ChapterBreak {
    pub file: usize,
    pub at: f64,
    pub chapter: usize,
    pub score: f64,
    pub before: String,
    pub after: String,
}

#[derive(Debug)]
pub struct ChapterMatch {
    pub breaks: Vec<ChapterBreak>,
    pub missing: Vec<usize>, // Chapters with no matching break.
}

// What to write for one input file: segment end times and optional file names.
#[derive(Debug, PartialEq)]
pub struct SplitJob {
    pub path: PathBuf,
    pub points: Vec<f64>,
    pub names: Option<Vec<String>>,
}

// File starts plus every long-enough silence, in playback order.
pub fn candidates(files: &[AudioFile], min_silence: f64) -> Vec<Candidate> {
    let mut out = Vec::new();
    for (i, f) in files.iter().enumerate() {
        out.push(Candidate { file: i, start: 0.0, end: 0.0 });
        out.extend(
            f.silences
                .iter()
                .filter(|s| s.duration >= min_silence && s.start >= EDGE_SECONDS && s.end <= f.duration - EDGE_SECONDS)
                .map(|s| Candidate { file: i, start: s.start, end: s.end }),
        );
    }
    out
}

// Transcribes the speech before and after each candidate. Returns (before, after) text.
pub fn transcribe_candidates(
    files: &[AudioFile],
    cands: &[Candidate],
    transcriber: &mut Transcriber,
    mut progress: impl FnMut(usize, usize),
) -> Result<Vec<(String, String)>, String> {
    let mut clip = |file: &AudioFile, from: f64, to: f64| -> Result<String, String> {
        if to - from < 0.5 {
            return Ok(String::new());
        }
        transcriber.transcribe(&clip_samples(&file.path, from, to - from)?)
    };
    let mut out = Vec::new();
    for (i, c) in cands.iter().enumerate() {
        let file = &files[c.file];
        let before = clip(file, (c.start - CLIP_SECONDS).max(0.0), c.start)?;
        let after = clip(file, c.end, (c.end + CLIP_SECONDS).min(file.duration))?;
        out.push((before, after));
        progress(i + 1, cands.len());
    }
    Ok(out)
}

// The part of a chapter a clip of `n` words should line up with: barely more than
// the clip itself, so a pause partway into a chapter doesn't match its opening.
fn edge_len(n: usize) -> usize {
    n + n / 5 + 10
}

// Picks one candidate per chapter (in order). A candidate qualifies when the speech
// after it matches the chapter's opening at `threshold` or better; a matching
// ending of the previous chapter before it adds to the score.
pub fn pick_breaks(
    cands: &[Candidate],
    transcripts: &[(String, String)],
    chapters: &[Chapter],
    threshold: f64,
) -> ChapterMatch {
    let words: Vec<(Vec<String>, Vec<String>)> =
        transcripts.iter().map(|(b, a)| (normalize(b), normalize(a))).collect();
    let score = |ch: usize, c: usize| -> Option<f64> {
        let (before, after) = &words[c];
        let head = &chapters[ch].words[..edge_len(after.len()).min(chapters[ch].words.len())];
        let after_score = bigram_score(after, head);
        if after_score < threshold {
            return None;
        }
        let before_score = ch.checked_sub(1).map_or(0.0, |prev| {
            let w = &chapters[prev].words;
            bigram_score(before, &w[w.len().saturating_sub(edge_len(before.len()))..])
        });
        Some(after_score + before_score)
    };

    let assigned = align(chapters.len(), cands.len(), &score);
    let mut breaks = Vec::new();
    let mut missing = Vec::new();
    for (ch, c) in assigned.into_iter().enumerate() {
        match c {
            Some(c) => breaks.push(ChapterBreak {
                file: cands[c].file,
                at: cands[c].split_at(),
                chapter: ch,
                score: score(ch, c).unwrap_or(0.0),
                before: transcripts[c].0.trim().to_string(),
                after: transcripts[c].1.trim().to_string(),
            }),
            None => missing.push(ch),
        }
    }
    ChapterMatch { breaks, missing }
}

// Where a chapter starts, as edited in the GUI. `file` indexes the analyzed files.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Break {
    pub file: usize,
    pub at: f64,
    pub title: String,
}

// Matched breaks as titled chapter starts, with an "Opening" for any audio before the first.
pub fn chapter_breaks(m: &ChapterMatch, chapters: &[Chapter]) -> Vec<Break> {
    let mut out: Vec<Break> = m
        .breaks
        .iter()
        .map(|b| Break {
            file: b.file,
            at: b.at,
            title: chapters[b.chapter].title.clone().unwrap_or_else(|| format!("Section {}", b.chapter + 1)),
        })
        .collect();
    if !out.first().is_some_and(|b| b.file == 0 && b.at <= 0.01) {
        out.insert(0, Break { file: 0, at: 0.0, title: "Opening".into() });
    }
    out
}

// Without an EPUB: a break at every file start and long-enough silence.
pub fn silence_breaks(files: &[AudioFile], min_silence: f64) -> Vec<Break> {
    candidates(files, min_silence)
        .iter()
        .enumerate()
        .map(|(i, c)| Break { file: c.file, at: c.split_at(), title: format!("Part {}", i + 1) })
        .collect()
}

// Turns chapter starts into split jobs, one per file. Each segment is named after its
// chapter; a chapter running on into the next file gets "(part 2)" and so on.
pub fn plan_from_breaks(files: &[AudioFile], breaks: &[Break]) -> Result<Vec<SplitJob>, String> {
    let mut breaks = breaks.to_vec();
    for b in &mut breaks {
        let duration = files.get(b.file).ok_or("A chapter break points at an unknown file.")?.duration;
        if !(b.at >= 0.0 && b.at < duration - 0.01) {
            return Err(format!("'{}' starts outside its audio file.", b.title));
        }
        if b.at <= 0.01 {
            b.at = 0.0;
        }
    }
    breaks.sort_by(|a, b| (a.file, a.at).partial_cmp(&(b.file, b.at)).unwrap());
    if let Some(w) = breaks.windows(2).find(|w| w[0].file == w[1].file && w[1].at - w[0].at <= 0.01) {
        return Err(format!("'{}' and '{}' start at the same time.", w[0].title, w[1].title));
    }

    let mut current: Option<String> = None; // Title of the chapter still playing.
    let mut part = 1;
    let mut jobs = Vec::new();
    for (i, f) in files.iter().enumerate() {
        let file_breaks: Vec<&Break> = breaks.iter().filter(|b| b.file == i).collect();
        let mut points = Vec::new();
        let mut names = Vec::new();
        if !file_breaks.first().is_some_and(|b| b.at == 0.0) {
            match &current {
                Some(title) => {
                    part += 1;
                    names.push(format!("{} (part {})", title, part));
                }
                None => {
                    names.push("Opening".to_string());
                    current = Some("Opening".to_string());
                }
            }
        }
        for b in file_breaks {
            if b.at > 0.0 {
                points.push(b.at);
            }
            names.push(b.title.trim().to_string());
            current = Some(b.title.trim().to_string());
            part = 1;
        }
        points.push(f.duration);
        jobs.push(SplitJob { path: f.path.clone(), points, names: Some(names) });
    }
    Ok(jobs)
}

// Makes a chapter title safe to use as a file name.
pub fn sanitize_file_name(name: &str) -> String {
    let name: String = name
        .trim_matches(|c: char| c.is_whitespace() || c == '.')
        .replace(':', " -")
        .chars()
        .map(|c| if c.is_control() || "/\\*?\"<>|".contains(c) { '_' } else { c })
        .take(120)
        .collect();
    if name.is_empty() { "Untitled".to_string() } else { name }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sil(start: f64, end: f64) -> Silence {
        Silence { start, end, duration: end - start }
    }

    fn file(name: &str, silences: Vec<Silence>, duration: f64) -> AudioFile {
        AudioFile { path: PathBuf::from(name), silences, duration }
    }

    fn chapter(title: &str, text: &str) -> Chapter {
        Chapter { title: Some(title.to_string()), words: normalize(text) }
    }

    fn c(file: usize, start: f64, end: f64) -> Candidate {
        Candidate { file, start, end }
    }

    #[test]
    fn candidates_include_file_starts_and_skip_edges_and_short_silences() {
        let files = [
            file("a.mp3", vec![sil(0.0, 2.5), sil(10.0, 13.0), sil(20.0, 20.5), sil(97.5, 100.0)], 100.0),
            file("b.mp3", vec![sil(30.0, 33.0)], 60.0),
        ];
        assert_eq!(
            candidates(&files, 2.0),
            vec![c(0, 0.0, 0.0), c(0, 10.0, 13.0), c(1, 0.0, 0.0), c(1, 30.0, 33.0)]
        );
    }

    const CH1: &str = "Chapter One. The morning fog rolled over the harbor as the fishing boats returned with their nets full of silver herring and the gulls screamed overhead. \
        Old Tom stood at the end of the pier and counted every boat twice. The last one came in late and empty.";
    const CH2: &str = "Chapter Two. Nobody in the village could remember a winter as cold as that one. The well froze solid in December \
        and the children skated across the mill pond while their parents carried water up from the river in buckets.";

    fn transcripts(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs.iter().map(|(b, a)| (b.to_string(), a.to_string())).collect()
    }

    #[test]
    fn pick_breaks_finds_chapter_starts_and_ignores_mid_chapter_silences() {
        let chapters = [
            chapter("Copyright", "All rights reserved. No part of this book may be reproduced in any form without permission from the publisher."),
            chapter("Chapter One", CH1),
            chapter("Chapter Two", CH2),
        ];
        let cands = [c(0, 0.0, 0.0), c(0, 40.0, 43.0), c(0, 90.0, 93.0)];
        let t = transcripts(&[
            ("", "Chapter 1. The morning fog rolled over the harbour as the fishing boats returned with their nets full of silver herring."),
            ("as the fishing boats returned", "Old Tom stood at the end of the pier and counted every boat twice."), // mid-chapter pause
            ("Old Tom stood at the end of the pier and counted every boat twice. The last one came in late and empty.",
             "Chapter two. Nobody in the village could remember a winter as cold as that one. The well froze solid in December."),
        ]);
        let m = pick_breaks(&cands, &t, &chapters, 0.4);
        let found: Vec<(usize, usize)> = m.breaks.iter().map(|b| (b.chapter, cands.iter().position(|x| x.split_at() == b.at).unwrap())).collect();
        assert_eq!(found, vec![(1, 0), (2, 2)]);
        assert_eq!(m.missing, vec![0]);
        assert!(m.breaks[1].score > 1.0, "before + after should both count: {}", m.breaks[1].score);
    }

    fn brk(file: usize, at: f64, chapter: usize) -> ChapterBreak {
        ChapterBreak { file, at, chapter, score: 1.0, before: String::new(), after: String::new() }
    }

    fn b(file: usize, at: f64, title: &str) -> Break {
        Break { file, at, title: title.to_string() }
    }

    fn names(v: &[&str]) -> Option<Vec<String>> {
        Some(v.iter().map(|s| s.to_string()).collect())
    }

    #[test]
    fn chapter_breaks_add_an_opening_and_name_untitled_chapters() {
        let chapters = [chapter("Copyright", "x"), Chapter { title: None, words: vec![] }, chapter("Part 2", "x")];
        let m = ChapterMatch { breaks: vec![brk(0, 5.0, 1), brk(1, 60.0, 2)], missing: vec![0] };
        assert_eq!(
            chapter_breaks(&m, &chapters),
            vec![b(0, 0.0, "Opening"), b(0, 5.0, "Section 2"), b(1, 60.0, "Part 2")]
        );
        // Book starts right on a chapter: no opening.
        let m = ChapterMatch { breaks: vec![brk(0, 0.0, 0)], missing: vec![] };
        assert_eq!(chapter_breaks(&m, &chapters), vec![b(0, 0.0, "Copyright")]);
    }

    #[test]
    fn silence_breaks_split_at_every_candidate() {
        let files = [file("a.mp3", vec![sil(10.0, 13.0), sil(20.0, 20.5)], 100.0), file("b.mp3", vec![sil(30.0, 34.0)], 60.0)];
        assert_eq!(
            silence_breaks(&files, 2.0),
            vec![b(0, 0.0, "Part 1"), b(0, 11.5, "Part 2"), b(1, 0.0, "Part 3"), b(1, 32.0, "Part 4")]
        );
    }

    #[test]
    fn plan_from_breaks_names_segments_and_continues_across_files() {
        let files = [file("a.mp3", vec![], 100.0), file("b.mp3", vec![], 80.0), file("c.mp3", vec![], 50.0)];
        // Unsorted on purpose; the Fall runs from mid a through b into c.
        let breaks = [b(0, 60.0, "The Fall"), b(0, 0.0, "Opening"), b(0, 5.0, "Chapter One")];
        assert_eq!(
            plan_from_breaks(&files, &breaks),
            Ok(vec![
                SplitJob { path: "a.mp3".into(), points: vec![5.0, 60.0, 100.0], names: names(&["Opening", "Chapter One", "The Fall"]) },
                SplitJob { path: "b.mp3".into(), points: vec![80.0], names: names(&["The Fall (part 2)"]) },
                SplitJob { path: "c.mp3".into(), points: vec![50.0], names: names(&["The Fall (part 3)"]) },
            ])
        );
    }

    #[test]
    fn plan_from_breaks_uses_file_start_breaks() {
        let files = [file("a.mp3", vec![], 100.0), file("b.mp3", vec![], 80.0)];
        let breaks = [b(0, 0.0, "One"), b(1, 0.0, "Two"), b(1, 40.0, "Three")];
        assert_eq!(
            plan_from_breaks(&files, &breaks),
            Ok(vec![
                SplitJob { path: "a.mp3".into(), points: vec![100.0], names: names(&["One"]) },
                SplitJob { path: "b.mp3".into(), points: vec![40.0, 80.0], names: names(&["Two", "Three"]) },
            ])
        );
    }

    #[test]
    fn plan_from_breaks_rejects_bad_breaks() {
        let files = [file("a.mp3", vec![], 100.0)];
        for bad in [
            vec![b(1, 0.0, "x")],                     // No such file.
            vec![b(0, -1.0, "x")],                    // Before the start.
            vec![b(0, 100.0, "x")],                   // At the end: empty segment.
            vec![b(0, f64::NAN, "x")],
            vec![b(0, 0.0, "x"), b(0, 50.0, "y"), b(0, 50.0, "z")], // Duplicate.
        ] {
            assert!(plan_from_breaks(&files, &bad).is_err(), "{:?}", bad);
        }
    }

    #[test]
    fn sanitize_file_name_removes_path_characters() {
        assert_eq!(sanitize_file_name("Part 2: The Fall"), "Part 2 - The Fall");
        assert_eq!(sanitize_file_name(" a/b\\c*?\"<>|\n "), "a_b_c______");
        assert_eq!(sanitize_file_name("..."), "Untitled");
        assert_eq!(sanitize_file_name(&"x".repeat(300)).len(), 120);
    }
}
