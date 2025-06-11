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

    // Main loop to allow the user to process multiple files.
    while process_another {
        // Call the core function to process a single audio file.
        match process_audio_file() {
            Ok(_) => println!("\nSuccessfully completed processing for this file."),
            Err(e) => eprintln!("\nAn error occurred during processing: {}", e),
        }

        // Prompt the user if they want to process another file.
        print!("\nDo you want to process another file? (y/n): ");
        // Ensure the prompt is displayed immediately by flushing the output buffer.
        io::stdout().flush().unwrap();
        let mut input = String::new();
        // Read the user's response.
        io::stdin().read_line(&mut input).unwrap();
        // Set 'process_another' based on whether the user typed 'y' (case-insensitive).
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


// Function to handle the entire process of detecting silences and splitting an audio file.
fn process_audio_file() -> Result<(), String> {
    // --- 1. Get audio file path from the user ---
    let input_audio_path = loop {
        print!("Enter the path to the audio file (e.g., audio.mp3 or C:\\path\\to\\audio.wav): ");
        io::stdout().flush().map_err(|e| format!("Failed to flush stdout: {}", e))?;
        let mut path_str = String::new();
        io::stdin().read_line(&mut path_str).map_err(|e| format!("Failed to read line: {}", e))?;
        let path = PathBuf::from(path_str.trim());

        // Validate if the entered path points to an existing file.
        if path.is_file() {
            break path;
        } else {
            println!("Error: File not found or is not a valid file. Please try again.");
        }
    };

    let mut silence_threshold_seconds: f64;
    let mut noise_threshold_db: f64;
    let mut split_points: Vec<f64> = Vec::new(); // Initialize here, will be updated in the loop

    // Loop for silence detection and re-analysis
    loop {
        // --- 2. Get the minimum silence length threshold from the user ---
        silence_threshold_seconds = loop {
            print!("Enter the minimum silence length in seconds (e.g., 2.0): ");
            io::stdout().flush().map_err(|e| format!("Failed to flush stdout: {}", e))?;
            let mut threshold_str = String::new();
            io::stdin().read_line(&mut threshold_str).map_err(|e| format!("Failed to read line: {}", e))?;

            // Parse the input string to a floating-point number and validate it.
            match threshold_str.trim().parse::<f64>() {
                Ok(t) if t > 0.0 => break t, // Accept only positive numbers.
                _ => println!("Error: Invalid threshold. Please enter a positive number."),
            }
        };

        // --- 3. Get the noise threshold (n value) from the user ---
        // This allows the user to fine-tune what FFmpeg considers "silence".
        // A less negative value (e.g., -30dB) means FFmpeg will be more lenient,
        // considering sounds up to that level as part of a silence.
        noise_threshold_db = loop {
            print!("Enter the noise threshold in dB (e.g., -40.0). Suggestion: -40.0dB. Less negative values detect more silence: ");
            io::stdout().flush().map_err(|e| format!("Failed to flush stdout: {}", e))?;
            let mut noise_str = String::new();
            io::stdin().read_line(&mut noise_str).map_err(|e| format!("Failed to read line: {}", e))?;

            // Parse the input string to a floating-point number.
            // It's common for this to be a negative number, so we only validate it's a number.
            match noise_str.trim().parse::<f64>() {
                Ok(n) => break n,
                _ => println!("Error: Invalid noise threshold. Please enter a number (e.g., -40.0)."),
            }
        };

        println!("\nStatus: Detecting silences in '{}' with threshold {:.2}s and noise {}dB...",
                 input_audio_path.display(), silence_threshold_seconds, noise_threshold_db);
        println!("(This might take a while for long audio files)");

        // --- 5. Detect silences using FFmpeg's 'silencedetect' filter ---
        let output = Command::new("ffmpeg")
            .arg("-i")
            .arg(&input_audio_path)
            .arg("-hide_banner") // Hide FFmpeg version and build config info
            .arg("-loglevel")   // Set logging level for detailed output
            .arg("debug")       // Use 'debug' to see full silencedetect output
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

        // Iterate line by line to capture start and end points correctly
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

        // --- 6. Determine effective split points based on the user's threshold ---
        split_points.clear(); // Clear previous split points for re-analysis
        for silence in detected_silences { // Use the newly detected silences
            if silence.duration >= silence_threshold_seconds {
                // Calculate the split point to be in the middle of the silence
                let mid_silence_point = silence.start + (silence.duration / 2.0);
                split_points.push(mid_silence_point);
            }
        }

        if split_points.is_empty() {
            println!("No silences detected longer than the specified threshold with current settings.");
            // Ask user if they want to re-analyze or exit if no splits found
            print!("Do you want to re-analyze with different settings? (y/n): ");
            io::stdout().flush().map_err(|e| format!("Failed to flush stdout: {}", e))?;
            let mut reanalyze_response = String::new();
            io::stdin().read_line(&mut reanalyze_response).map_err(|e| format!("Failed to read line: {}", e))?;
            if reanalyze_response.trim().eq_ignore_ascii_case("y") {
                continue; // Restart the loop for new settings
            } else {
                return Ok(()); // Exit if user doesn't want to re-analyze and no splits found
            }
        }

        println!("Status: Identified {} audio segments to be split.", split_points.len());

        // --- Prompt for re-analysis or proceed ---
        print!("Do you want to (r)e-analyze with different settings or (p)roceed to split? (r/p): ");
        io::stdout().flush().map_err(|e| format!("Failed to flush stdout: {}", e))?;
        let mut choice = String::new();
        io::stdin().read_line(&mut choice).map_err(|e| format!("Failed to read line: {}", e))?;

        match choice.trim().to_lowercase().as_str() {
            "r" => continue, // Restart the loop for re-analysis
            "p" => break,    // Break out of the analysis loop to proceed with splitting
            _ => {
                println!("Invalid choice. Please enter 'r' to re-analyze or 'p' to proceed. Re-analyzing by default...");
                continue; // Default to re-analyze on invalid input
            }
        }
    }

    // --- 4. Get the output path and file name prefix from the user ---
    let output_prefix = loop {
        print!("Enter the output directory and file name prefix (e.g., output/part): ");
        io::stdout().flush().map_err(|e| format!("Failed to flush stdout: {}", e))?;
        let mut prefix_str = String::new();
        io::stdin().read_line(&mut prefix_str).map_err(|e| format!("Failed to read line: {}", e))?;
        let prefix = prefix_str.trim().to_string();

        if prefix.is_empty() {
            println!("Error: Output prefix cannot be empty.");
        } else {
            // Extract the directory part from the prefix. If no directory is specified,
            // assume the current directory.
            let output_dir = PathBuf::from(&prefix).parent().unwrap_or(Path::new(".")).to_path_buf();
            if !output_dir.exists() {
                // If the output directory doesn't exist, ask the user to create it.
                print!("Output directory '{}' does not exist. Create it? (y/n): ", output_dir.display());
                io::stdout().flush().map_err(|e| format!("Failed to flush stdout: {}", e))?;
                let mut create_dir_response = String::new();
                io::stdin().read_line(&mut create_dir_response).map_err(|e| format!("Failed to read line: {}", e))?;
                if create_dir_response.trim().eq_ignore_ascii_case("y") {
                    std::fs::create_dir_all(output_dir.clone())
                        .map_err(|e| format!("Failed to create directory '{}': {}", output_dir.display(), e))?;
                    break prefix; // Directory created, so the prefix is valid.
                } else {
                    println!("Cannot proceed without a valid output directory.");
                }
            } else {
                break prefix; // Directory exists, so the prefix is valid.
            }
        }
    };

    // Get the total duration of the input audio file using ffprobe.
    // This is crucial to ensure the last segment correctly goes to the very end of the file.
    let total_duration_output = Command::new("ffprobe")
        .arg("-v") // Verbosity level (error messages only)
        .arg("error")
        .arg("-show_entries") // Show specific entries
        .arg("format=duration") // Show format duration
        .arg("-of") // Output format
        .arg("default=noprint_wrappers=1:nokey=1") // Clean output: just the value
        .arg(&input_audio_path)
        .stdout(Stdio::piped()) // Capture stdout
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

    // Ensure the last segment of the audio is always included.
    // If there are no split points yet, or if the last split point doesn't reach the end of the file,
    // add the total duration as a final split point.
    // This check is important as split points are mid-silence, ensuring the end of the file is covered.
    if split_points.is_empty() || split_points.last().map_or(false, |&last_split| last_split < total_duration - 0.01) {
        split_points.push(total_duration);
    }


    // --- 7. Split audio using FFmpeg for each determined segment ---
    let mut current_segment_start_time = 0.0; // Start time for the first segment.

    // Get the file extension of the original audio for the output files.
    let output_file_extension = input_audio_path
        .extension()
        .unwrap_or_default() // If no extension, use empty string
        .to_string_lossy()
        .to_string();

    // Determine the starting index for new files to avoid overwriting.
    let mut file_index = get_next_file_index(&output_prefix, &output_file_extension)?;
    println!("Status: Starting new split files from index {}.", file_index);


    for (i, &split_end_time) in split_points.iter().enumerate() {
        // Calculate the duration of the current segment.
        let duration = split_end_time - current_segment_start_time;

        // Skip if the duration is non-positive (e.g., due to consecutive split points too close).
        if duration <= 0.01 { // Use a small epsilon for float comparison
            current_segment_start_time = split_end_time;
            continue;
        }

        // Construct the output file name (e.g., output/part_001.mp3).
        // Use `file_index` for naming to ensure no overwrites.
        let output_file_name = format!("{}_{:03}.{}", output_prefix, file_index, output_file_extension);

        println!("Status: Splitting part {} (from {:.2}s to {:.2}s, duration {:.2}s) to '{}'...",
                 i + 1, current_segment_start_time, split_end_time, duration, output_file_name);

        // Execute FFmpeg to extract the current segment.
        let status = Command::new("ffmpeg")
            .arg("-i") // Input file
            .arg(&input_audio_path)
            .arg("-ss") // Start seek (position to start from)
            .arg(format!("{}", current_segment_start_time))
            .arg("-t") // Duration (length of the segment to extract)
            .arg(format!("{}", duration))
            .arg("-c") // Codec (use 'copy' to avoid re-encoding for speed and quality)
            .arg("copy") // Stream copy for audio and video if applicable.
            .arg("-y") // Overwrite output files without asking (overwrite temp files, not existing indexed files)
            .arg(&output_file_name)
            .status() // Get the exit status of the command.
            .map_err(|e| format!("Failed to execute ffmpeg for splitting. Error: {}", e))?;

        // Check if the splitting command was successful.
        if !status.success() {
            return Err(format!("FFmpeg splitting failed for part {}. Status: {}", i + 1, status));
        }

        // Update the start time for the next segment.
        current_segment_start_time = split_end_time;
        file_index += 1; // Increment for the next file
    }

    Ok(()) // Indicate success.
}
