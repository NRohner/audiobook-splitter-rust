// Per-book tags and track naming: every output file is one track of disc 1.
use crate::chapters::{sanitize_file_name, SplitJob};
use serde::{Deserialize, Serialize};

pub const DEFAULT_TEMPLATE: &str = "{n} - {title}";

// Empty fields keep whatever tag the source file had.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Metadata {
    pub album: String,
    pub artist: String,
    pub album_artist: String,
    pub genre: String,
    pub year: String,
    pub template: String, // File name and title, with {n}, {title} and {album}.
}

// One output file: its name (without extension) and the tags to write.
#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    pub name: String,
    pub tags: Vec<(String, String)>,
}

// Names and tags every segment of the plan, in order. `{n}` counts from 00 (books
// nearly always open with an intro); the track tag counts from 1, as players expect.
pub fn tracks(plan: &[SplitJob], meta: &Metadata) -> Result<Vec<Vec<Track>>, String> {
    if meta.template.trim().is_empty() {
        return Err("Enter a track naming convention, such as {n} - {title}.".into());
    }
    let total: usize = plan.iter().map(|job| job.points.len()).sum();
    let width = total.saturating_sub(1).to_string().len().max(2);
    let shared = [
        ("album", &meta.album),
        ("artist", &meta.artist),
        ("album_artist", &meta.album_artist),
        ("genre", &meta.genre),
        ("date", &meta.year),
    ];
    let mut n = 0;
    Ok(plan
        .iter()
        .map(|job| {
            (0..job.points.len())
                .map(|i| {
                    let title = job.names.as_ref().and_then(|names| names.get(i)).map_or("", String::as_str);
                    // {title} last, so a title containing "{n}" stays as written.
                    let name = meta
                        .template
                        .replace("{n}", &format!("{:0width$}", n))
                        .replace("{album}", meta.album.trim())
                        .replace("{title}", title)
                        .trim()
                        .to_string();
                    let mut tags = vec![
                        ("title".to_string(), name.clone()),
                        ("track".to_string(), format!("{}/{}", n + 1, total)),
                        ("disc".to_string(), "1/1".to_string()),
                    ];
                    tags.extend(
                        shared.iter().filter(|(_, v)| !v.trim().is_empty()).map(|(k, v)| (k.to_string(), v.trim().to_string())),
                    );
                    n += 1;
                    Track { name: sanitize_file_name(&name), tags }
                })
                .collect()
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(titles: &[&str]) -> SplitJob {
        SplitJob {
            path: "a.mp3".into(),
            points: (1..=titles.len()).map(|i| i as f64).collect(),
            names: Some(titles.iter().map(|s| s.to_string()).collect()),
        }
    }

    fn meta(template: &str) -> Metadata {
        Metadata { album: "Moby: Dick".into(), artist: "Melville".into(), template: template.into(), ..Default::default() }
    }

    #[test]
    fn tracks_number_from_zero_across_files_on_disc_one() {
        let plan = [job(&["Opening", "Chapter: One"]), job(&["Chapter: One (part 2)"])];
        let tracks = tracks(&plan, &meta(DEFAULT_TEMPLATE)).unwrap();
        let names: Vec<Vec<&str>> = tracks.iter().map(|t| t.iter().map(|t| t.name.as_str()).collect()).collect();
        assert_eq!(names, [vec!["00 - Opening", "01 - Chapter - One"], vec!["02 - Chapter - One (part 2)"]]);

        let tag = |t: &Track, k: &str| t.tags.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone());
        let second = &tracks[0][1];
        assert_eq!(tag(second, "title").as_deref(), Some("01 - Chapter: One"));
        assert_eq!(tag(second, "track").as_deref(), Some("2/3"));
        assert_eq!(tag(second, "disc").as_deref(), Some("1/1"));
        assert_eq!(tag(second, "album").as_deref(), Some("Moby: Dick"));
        assert_eq!(tag(second, "artist").as_deref(), Some("Melville"));
        assert_eq!(tag(second, "genre"), None, "empty fields keep the source tag");
    }

    #[test]
    fn template_tokens_and_padding() {
        let titles: Vec<String> = (0..120).map(|i| format!("T{}", i)).collect();
        let titles: Vec<&str> = titles.iter().map(String::as_str).collect();
        let tracks = tracks(&[job(&titles)], &meta("{album} {n}. {title}")).unwrap();
        assert_eq!(tracks[0][0].name, "Moby - Dick 000. T0");
        assert_eq!(tracks[0][119].name, "Moby - Dick 119. T119");
    }

    #[test]
    fn empty_template_is_an_error() {
        assert!(tracks(&[job(&["a"])], &meta("  ")).is_err());
    }
}
