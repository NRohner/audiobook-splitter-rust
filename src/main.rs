// Import necessary modules from the standard library and external crates.
use std::io::{self, Write}; // For input/output operations (reading user input, printing to console).
use std::process::{Command, Stdio}; // For running external commands (FFmpeg, FFprobe).
use std::path::{Path, PathBuf}; // For working with file paths.
use regex::Regex; // For parsing FFmpeg's silence detection output.

// Define a struct to hold the details of a detected silence region.
struct Silence {
    start: f64,    // The starting timestamp of the silence in seconds.
    end: f64,      // The ending timestamp of the silence in seconds.
    duration: f64, // The duration of the silence in seconds.
}

fn main() {
    println!("Welcome to the Audio Splitter!");
    println!("--------------------------------");
    println!("Note: This application requires FFmpeg and FFprobe to be installed");
    println!("      and accessible in your system's PATH for audio processing.");
    println!("--------------------------------");

    let mut process_another = true;

    // Main loop to allow the user to process multiple files or batches.
    while process_another {
        let mut input_paths: Vec<PathBuf> = Vec::new();
        let output_base_dir: PathBuf;

        // Prompt user to choose between single file or folder processing
        let process_type = loop {
            print!("Do you want to process a (s)ingle file or a (f)older of files? (s/f): ");
            io::stdout().flush().unwrap();
            let mut choice = String::new();
            io::stdin().read_line(&mut choice).unwrap();
            let choice_trimmed = choice.trim().to_lowercase();
            if choice_trimmed == "s" || choice_trimmed == "f" {
                break choice_trimmed;
            } else {
                println!("Invalid choice. Please enter 's' or 'f'.");
            }
        };

        // Get the input audio path(s) based on user's choice
        if process_type == "s" {
            // Get single audio file path
            let path = loop {
                print!("Enter the path to the audio file (e.g., audio.mp3 or C:\\path\\to\\audio.wav): ");
                io::stdout().flush().unwrap();
                let mut path_str = String::new();
                io::stdin().read_line(&mut path_str).unwrap();
                let p = PathBuf::from(path_str.trim());
                if p.is_file() {
                    break p;
                } else {
                    println!("Error: File not found or is not a valid file. Please try again.");
                }
            };
            input_paths.push(path);
        } else { // process_type == "f"
            // Get folder path and collect audio files
            let folder_path = loop {
                print!("Enter the path to the folder containing audio files: ");
                io::stdout().flush().unwrap();
                let mut path_str = String::new();
                io::stdin().read_line(&mut path_str).unwrap();
                let p = PathBuf::from(path_str.trim());
                if p.is_dir() {
                    break p;
                } else {
                    println!("Error: Folder not found or is not a valid directory. Please try again.");
                }
            };

            println!("Status: Scanning folder '{}' for audio files...", folder_path.display());
            let audio_extensions = ["mp3", "wav", "flac", "aac", "m4a", "ogg"]; // Common audio extensions
            for entry in std::fs::read_dir(&folder_path).expect("Failed to read directory") {
                let entry = entry.expect("Failed to read directory entry");
                let path = entry.path();
                if path.is_file() {
                    if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                        if audio_extensions.contains(&ext.to_lowercase().as_str()) {
                            input_paths.push(path);
                        }
                    }
                }
            }
            input_paths.sort_by(|a, b| {
                a.file_name().cmp(&b.file_name()) // Sort alphabetically by filename
            });

            if input_paths.is_empty() {
                println!("No supported audio files found in the specified folder. Please try again.");
                process_another = true; // Loop back to ask if another file/folder
                continue;
            }
            println!("Status: Found {} audio files in the folder.", input_paths.len());
        }

        // Get the base output directory (will be used for all splits)
        output_base_dir = loop {
            print!("Enter the base output directory (e.g., output_splits or C:\\MyAudioSplits): ");
            io::stdout().flush().unwrap();
            let mut dir_str = String::new();
            io::stdin().read_line(&mut dir_str).unwrap();
            let dir_path = PathBuf::from(dir_str.trim());

            if !dir_path.exists() {
                print!("Output directory '{}' does not exist. Create it? (y/n): ", dir_path.display());
                io::stdout().flush().unwrap();
                let mut create_dir_response = String::new();
                io::stdin().read_line(&mut create_dir_response).unwrap();
                if create_dir_response.trim().eq_ignore_ascii_case("y") {
                    if let Err(e) = std::fs::create_dir_all(&dir_path) {
                        eprintln!("Failed to create directory '{}': {}", dir_path.display(), e);
                        continue; // Ask for output directory again
                    }
                    break dir_path; // Directory created, so the path is valid.
                } else {
                    println!("Cannot proceed without a valid output directory.");
                    continue; // Ask for output directory again
                }
            } else if !dir_path.is_dir() {
                println!("Error: The provided path is not a directory. Please enter a valid directory path.");
            }
            else {
                break dir_path; // Directory exists and is valid.
            }
        };

        // --- Parameters for analysis, prompted once for single file or entire folder batch ---
        let silence_threshold_seconds: f64 = loop {
            print!("Enter the minimum silence length in seconds (e.g., 2.0): ");
            io::stdout().flush().unwrap();
            let mut threshold_str = String::new();
            io::stdin().read_line(&mut threshold_str).unwrap();
            match threshold_str.trim().parse::<f64>() {
                Ok(t) if t > 0.0 => break t,
                _ => println!("Error: Invalid threshold. Please enter a positive number."),
            }
        };

        let noise_threshold_db: f64 = loop {
            print!("Enter the noise threshold in dB (e.g., -40.0). Suggestion: -40.0dB. Less negative values detect more silence: ");
            io::stdout().flush().unwrap();
            let mut noise_str = String::new();
            io::stdin().read_line(&mut noise_str).unwrap();
            match noise_str.trim().parse::<f64>() {
                Ok(n) => break n,
                _ => println!("Error: Invalid noise threshold. Please enter a number (e.g., -40.0)."),
            }
        };

        // If processing a single file, we offer re-analysis; for folders, we assume batch processing.
        let mut proceed_with_splitting = false;
        if process_type == "s" {
            loop {
                println!("\nStatus: Performing initial silence detection for '{}' with threshold {:.2}s and noise {}dB...",
                         input_paths[0].display(), silence_threshold_seconds, noise_threshold_db);
                println!("(This might take a while for long audio files)");

                let (detected_silences_for_single_file, total_duration_for_single_file) =
                    match detect_silences_and_get_total_duration(&input_paths[0], silence_threshold_seconds, noise_threshold_db) {
                        Ok((silences, duration)) => (silences, duration),
                        Err(e) => {
                            eprintln!("An error occurred during initial detection: {}", e);
                            // For a single file, if detection fails, allow re-analysis or exit.
                            print!("Do you want to re-analyze this file with different settings? (y/n): ");
                            io::stdout().flush().unwrap();
                            let mut reanalyze_response = String::new();
                            io::stdin().read_line(&mut reanalyze_response).unwrap();
                            if reanalyze_response.trim().eq_ignore_ascii_case("y") {
                                continue; // Restart the loop for new settings
                            } else {
                                // User chooses not to re-analyze, skip this file entirely.
                                // We'll set proceed_with_splitting to false and break out.
                                println!("Skipping splitting for '{}' due to detection error.", input_paths[0].display());
                                break;
                            }
                        }
                    };

                let mut temp_split_points: Vec<f64> = Vec::new();
                for silence in detected_silences_for_single_file {
                    if silence.duration >= silence_threshold_seconds {
                        temp_split_points.push(silence.start + (silence.duration / 2.0));
                    }
                }

                // Ensure the last segment of the audio is always included for accurate point count
                if temp_split_points.is_empty() || temp_split_points.last().map_or(false, |&last_split| last_split < total_duration_for_single_file - 0.01) {
                    temp_split_points.push(total_duration_for_single_file);
                }

                println!("Status: Identified {} audio segments to be split for '{}'.", temp_split_points.len(), input_paths[0].display());

                print!("Do you want to (r)e-analyze this file with different settings or (p)roceed to split? (r/p): ");
                io::stdout().flush().unwrap();
                let mut choice = String::new();
                io::stdin().read_line(&mut choice).unwrap();
                match choice.trim().to_lowercase().as_str() {
                    "r" => continue,
                    "p" => { proceed_with_splitting = true; break; },
                    _ => { println!("Invalid choice. Re-analyzing by default..."); continue; }
                }
            }
        } else { // process_type == "f" - default to proceed after asking settings once
            proceed_with_splitting = true; // For folder processing, we always proceed after settings are given
        }

        if proceed_with_splitting {
            // Process each audio file
            for audio_file_path in &input_paths {
                println!("\n--- Processing: {} ---", audio_file_path.display());
                match perform_analysis_and_split(
                    audio_file_path,
                    &output_base_dir,
                    silence_threshold_seconds,
                    noise_threshold_db
                ) {
                    Ok(_) => println!("Successfully completed processing for {}.", audio_file_path.display()),
                    Err(e) => eprintln!("An error occurred during processing {}: {}", audio_file_path.display(), e),
                }
            }
        } else {
            // If processing single file and user chose not to proceed after re-analysis prompt
            println!("Skipping audio splitting for the current file.");
        }


        // Prompt the user if they want to process another file/folder.
        print!("\nDo you want to process another file or folder? (y/n): ");
        io::stdout().flush().unwrap();
        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();
        process_another = input.trim().eq_ignore_ascii_case("y");
    }

    println!("\nThank you for using the Audio Splitter! Goodbye.");
}

// Helper function to determine the next available file index in a directory.
// It scans for files matching the output prefix and extension, extracts their numbers,
// and returns the highest number found + 1, or 1 if no matching files exist.
fn get_next_file_index(output_prefix: &str, output_file_extension: &str) -> Result<usize, String> {
    // Extract the directory part from the prefix. If no directory is specified,
    // assume the current directory.
    let output_dir = PathBuf::from(output_prefix).parent().unwrap_or(Path::new(".")).to_path_buf();
    let file_prefix_stem = PathBuf::from(output_prefix)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();

    // Construct a regex to match files like "prefix_001.ext", "prefix_002.ext", etc.
    // The `file_prefix_stem` needs to be escaped for regex special characters.
    let escaped_file_prefix_stem = regex::escape(&file_prefix_stem);
    let regex_pattern = format!(r"^{}_(?P<index>\d{{3,}})\.{}$", escaped_file_prefix_stem, regex::escape(output_file_extension));
    let file_regex = Regex::new(&regex_pattern)
        .map_err(|e| format!("Failed to create regex for file indexing: {}", e))?;

    let mut max_index = 0;

    if output_dir.exists() && output_dir.is_dir() {
        for entry in std::fs::read_dir(&output_dir)
            .map_err(|e| format!("Failed to read output directory '{}': {}", output_dir.display(), e))?
        {
            let entry = entry.map_err(|e| format!("Failed to read directory entry: {}", e))?;
            let path = entry.path();
            if path.is_file() {
                if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                    if let Some(captures) = file_regex.captures(file_name) {
                        if let Some(index_str) = captures.name("index") {
                            if let Ok(index) = index_str.as_str().parse::<usize>() {
                                if index > max_index {
                                    max_index = index;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(max_index + 1)
}

// New helper function to detect silences and get total duration from FFmpeg/FFprobe.
fn detect_silences_and_get_total_duration(
    input_audio_path: &PathBuf,
    silence_threshold_seconds: f64,
    noise_threshold_db: f64,
) -> Result<(Vec<Silence>, f64), String> {
    // --- Detect silences using FFmpeg's 'silencedetect' filter ---
    let output = Command::new("ffmpeg")
        .arg("-i")
        .arg(input_audio_path)
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("debug")
        .arg("-af")
        .arg(format!("silencedetect=n={}dB:d={}", noise_threshold_db, silence_threshold_seconds))
        .arg("-f")
        .arg("null")
        .arg("-")
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn ffmpeg. Please ensure FFmpeg is installed and in your PATH. Error: {}", e))?
        .wait_with_output()
        .map_err(|e| format!("Failed to wait for ffmpeg process: {}", e))?;

    if !output.status.success() {
        let stderr_str = String::from_utf8_lossy(&output.stderr);
        return Err(format!("FFmpeg exited with a non-zero status code during silence detection. Stderr:\n{}", stderr_str));
    }

    let stderr_str = String::from_utf8_lossy(&output.stderr);
    let re_start = Regex::new(r"silence_start: (?P<start>\d+\.\d+)").unwrap();
    let re_end = Regex::new(r"silence_end: (?P<end>\d+\.\d+) \| silence_duration: (?P<duration>\d+\.\d+)").unwrap();

    let mut starts: Vec<f64> = Vec::new();
    let mut detected_silences: Vec<Silence> = Vec::new();

    for line in stderr_str.lines() {
        if let Some(cap) = re_start.captures(line) {
            let start = cap["start"].parse::<f64>().map_err(|e| format!("Failed to parse silence start time: {}", e))?;
            starts.push(start);
        } else if let Some(cap) = re_end.captures(line) {
            let end = cap["end"].parse::<f64>().map_err(|e| format!("Failed to parse silence end time: {}", e))?;
            let duration = cap["duration"].parse::<f64>().map_err(|e| format!("Failed to parse silence duration: {}", e))?;

            if let Some(start) = starts.pop() {
                detected_silences.push(Silence { start, end, duration });
            } else {
                eprintln!("Warning: Found silence_end without a matching silence_start. End: {:.2}s, Duration: {:.2}s", end, duration);
            }
        }
    }

    // Get the total duration of the input audio file using ffprobe.
    let total_duration_output = Command::new("ffprobe")
        .arg("-v")
        .arg("error")
        .arg("-show_entries")
        .arg("format=duration")
        .arg("-of")
        .arg("default=noprint_wrappers=1:nokey=1")
        .arg(input_audio_path)
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn ffprobe. Please ensure FFprobe is installed and in your PATH. Error: {}", e))?
        .wait_with_output()
        .map_err(|e| format!("Failed to wait for ffprobe process: {}", e))?;

    if !total_duration_output.status.success() {
        let stderr_str = String::from_utf8_lossy(&total_duration_output.stderr);
        return Err(format!("FFprobe exited with a non-zero status code. Stderr:\n{}", stderr_str));
    }

    let total_duration_str = String::from_utf8_lossy(&total_duration_output.stdout);
    let total_duration = total_duration_str.trim().parse::<f64>().map_err(|e| format!("Failed to parse total audio duration: {}", e))?;

    Ok((detected_silences, total_duration))
}


// Function to handle the entire process of detecting silences and splitting a single audio file.
// Now takes input_audio_path, base_output_dir, silence_threshold_seconds, and noise_threshold_db as arguments.
fn perform_analysis_and_split(
    input_audio_path: &PathBuf,
    base_output_dir: &PathBuf,
    silence_threshold_seconds: f64,
    noise_threshold_db: f64,
) -> Result<(), String> {
    // No more prompts here; values are passed in.
    println!("  Status: Detecting silences in '{}' with threshold {:.2}s and noise {}dB...",
             input_audio_path.display(), silence_threshold_seconds, noise_threshold_db);
    println!("  (This might take a while for long audio files)");

    let (detected_silences, total_duration) = detect_silences_and_get_total_duration(
        input_audio_path,
        silence_threshold_seconds,
        noise_threshold_db,
    )?;

    let mut split_points: Vec<f64> = Vec::new();
    for silence in detected_silences {
        if silence.duration >= silence_threshold_seconds {
            let mid_silence_point = silence.start + (silence.duration / 2.0);
            split_points.push(mid_silence_point);
        }
    }

    if split_points.is_empty() {
        println!("  No silences detected longer than the specified threshold for '{}'. Skipping splitting for this file.", input_audio_path.display());
        return Ok(()); // No splits to make for this file
    }

    // Ensure the last segment of the audio is always included.
    if split_points.is_empty() || split_points.last().map_or(false, |&last_split| last_split < total_duration - 0.01) {
        split_points.push(total_duration);
    }

    println!("  Status: Identified {} audio segments to be split for '{}'.", split_points.len(), input_audio_path.display());

    // --- Split audio using FFmpeg for each determined segment ---
    let mut current_segment_start_time = 0.0;

    let output_file_extension = input_audio_path
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let file_stem = input_audio_path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("audio_part");

    let output_prefix = base_output_dir.join(file_stem).to_string_lossy().to_string();

    let mut file_index = get_next_file_index(&output_prefix, &output_file_extension)?;
    println!("  Status: Starting new split files for '{}' from index {}.", input_audio_path.display(), file_index);


    for (i, &split_end_time) in split_points.iter().enumerate() {
        let duration = split_end_time - current_segment_start_time;

        if duration <= 0.01 {
            current_segment_start_time = split_end_time;
            continue;
        }

        let output_file_name = format!("{}_{:03}.{}", output_prefix, file_index, output_file_extension);

        println!("  Status: Splitting part {} (from {:.2}s to {:.2}s, duration {:.2}s) to '{}'...",
                 i + 1, current_segment_start_time, split_end_time, duration, output_file_name);

        let status = Command::new("ffmpeg")
            .arg("-i")
            .arg(input_audio_path)
            .arg("-ss")
            .arg(format!("{}", current_segment_start_time))
            .arg("-t")
            .arg(format!("{}", duration))
            .arg("-c")
            .arg("copy")
            .arg("-y")
            .arg(&output_file_name)
            .status()
            .map_err(|e| format!("Failed to execute ffmpeg for splitting. Error: {}", e))?;

        if !status.success() {
            return Err(format!("FFmpeg splitting failed for part {}. Status: {}", i + 1, status));
        }

        current_segment_start_time = split_end_time;
        file_index += 1;
    }

    Ok(())
}
