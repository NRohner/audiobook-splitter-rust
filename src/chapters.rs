// Chapter-break detection: silences become candidates, Whisper transcribes the
// speech either side, and the transcripts are matched against the EPUB chapters.
use crate::book::Chapter;
use crate::matching::{align, bigram_score, normalize};
use crate::transcribe::{clip_samples, Transcriber};
use crate::Silence;
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

// Turns matched breaks into split jobs. Every file is written; segments are named
// "NN - Chapter Title" when the EPUB has titles, otherwise the standard numbering is used.
pub fn chapter_plan(files: &[AudioFile], m: &ChapterMatch, chapters: &[Chapter]) -> Vec<SplitJob> {
    let titled = chapters.iter().all(|c| c.title.is_some());
    let mut current: Option<String> = None; // Name of the segment still playing.
    let mut parts = 1;
    let mut jobs = Vec::new();

    for (i, f) in files.iter().enumerate() {
        let mut points = Vec::new();
        let mut names = Vec::new();
        let mut file_breaks = m.breaks.iter().enumerate().filter(|(_, b)| b.file == i).peekable();

        // The first segment either starts a chapter or continues the previous one.
        if !file_breaks.peek().is_some_and(|(_, b)| b.at <= 0.01) {
            let name = current.clone().unwrap_or_else(|| "00 - Opening".to_string());
            if i > 0 {
                parts += 1;
                names.push(format!("{} (part {})", name, parts));
            } else {
                names.push(name.clone());
            }
            current = Some(name);
        }
        for (n, b) in file_breaks {
            if b.at > 0.01 {
                points.push(b.at);
            }
            let title = chapters[b.chapter].title.as_deref().unwrap_or_default();
            let name = format!("{:02} - {}", n + 1, sanitize_file_name(title));
            names.push(name.clone());
            current = Some(name);
            parts = 1;
        }
        points.push(f.duration);
        jobs.push(SplitJob { path: f.path.clone(), points, names: titled.then_some(names) });
    }
    jobs
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

    #[test]
    fn chapter_plan_names_segments_and_continues_across_files() {
        let files = [file("a.mp3", vec![], 100.0), file("b.mp3", vec![], 80.0)];
        let chapters = [chapter("Copyright", "x"), chapter("Chapter One", "x"), chapter("Part 2: The Fall", "x")];
        // Chapter One starts 5s in (after an intro); the Fall starts mid file a and runs into b.
        let m = ChapterMatch { breaks: vec![brk(0, 5.0, 1), brk(0, 60.0, 2)], missing: vec![0] };
        assert_eq!(
            chapter_plan(&files, &m, &chapters),
            vec![
                SplitJob {
                    path: "a.mp3".into(),
                    points: vec![5.0, 60.0, 100.0],
                    names: Some(vec!["00 - Opening".into(), "01 - Chapter One".into(), "02 - Part 2 - The Fall".into()]),
                },
                SplitJob { path: "b.mp3".into(), points: vec![80.0], names: Some(vec!["02 - Part 2 - The Fall (part 2)".into()]) },
            ]
        );
    }

    #[test]
    fn chapter_plan_uses_file_start_break_and_standard_names_without_titles() {
        let files = [file("a.mp3", vec![], 100.0)];
        let chapters = [Chapter { title: None, words: vec![] }, Chapter { title: None, words: vec![] }];
        let m = ChapterMatch { breaks: vec![brk(0, 0.0, 0), brk(0, 50.0, 1)], missing: vec![] };
        assert_eq!(
            chapter_plan(&files, &m, &chapters),
            vec![SplitJob { path: "a.mp3".into(), points: vec![50.0, 100.0], names: None }]
        );
    }

    #[test]
    fn sanitize_file_name_removes_path_characters() {
        assert_eq!(sanitize_file_name("Part 2: The Fall"), "Part 2 - The Fall");
        assert_eq!(sanitize_file_name(" a/b\\c*?\"<>|\n "), "a_b_c______");
        assert_eq!(sanitize_file_name("..."), "Untitled");
        assert_eq!(sanitize_file_name(&"x".repeat(300)).len(), 120);
    }
}
