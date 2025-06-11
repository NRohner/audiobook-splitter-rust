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

    // --- 2. Get the minimum silence length threshold from the user ---
    let silence_threshold_seconds: f64 = loop {
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

    // --- 3. Get the output path and file name prefix from the user ---
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
                    // Attempt to create the directory and all its necessary parent directories.
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

    println!("\nStatus: Detecting silences in '{}'...", input_audio_path.display());
    println!("(This might take a while for long audio files)");

    // --- 4. Detect silences using FFmpeg's 'silencedetect' filter ---
    // The silencedetect filter outputs detection information to stderr.
    let output = Command::new("ffmpeg")
        .arg("-i") // Input file
        .arg(&input_audio_path)
        .arg("-af") // Audio filtergraph
        // silencedetect: n=-50dB (noise threshold - samples below this are considered silence),
        // d=silence_threshold_seconds (minimum duration for a detected silence to be reported).
        // A threshold of -50dB is common; you might adjust this if needed for very noisy/quiet audio.
        .arg(format!("silencedetect=n=-50dB:d={}", silence_threshold_seconds))
        .arg("-f") // Force format (null means no output file)
        .arg("null")
        .arg("-") // Send output to stdout (silencedetect logs to stderr)
        .stderr(Stdio::piped()) // Capture stderr to parse silence detection logs.
        .spawn() // Execute the command.
        .map_err(|e| format!("Failed to spawn ffmpeg. Please ensure FFmpeg is installed and in your PATH. Error: {}", e))?
        .wait_with_output() // Wait for FFmpeg to complete and capture its output.
        .map_err(|e| format!("Failed to wait for ffmpeg process: {}", e))?;

    // Check if FFmpeg command executed successfully.
    if !output.status.success() {
        let stderr_str = String::from_utf8_lossy(&output.stderr);
        return Err(format!("FFmpeg exited with a non-zero status code. Stderr:\n{}", stderr_str));
    }

    // Parse FFmpeg's stderr output to extract silence information.
    let stderr_str = String::from_utf8_lossy(&output.stderr);
    // Regular expression to match lines containing silence_start, silence_end, and silence_duration.
    let re = Regex::new(r"silence_start: (?P<start>\d+\.\d+) \| silence_end: (?P<end>\d+\.\d+) \| silence_duration: (?P<duration>\d+\.\d+)").unwrap();

    let mut silences: Vec<Silence> = Vec::new();
    // Iterate over all matches found by the regex.
    for cap in re.captures_iter(&stderr_str) {
        // Parse the captured strings into f64 (floating-point numbers).
        let start = cap["start"].parse::<f64>().map_err(|e| format!("Failed to parse silence start time: {}", e))?;
        let end = cap["end"].parse::<f64>().map_err(|e| format!("Failed to parse silence end time: {}", e))?;
        let duration = cap["duration"].parse::<f64>().map_err(|e| format!("Failed to parse silence duration: {}", e))?;
        silences.push(Silence { start, end, duration });
    }

    if silences.is_empty() {
        println!("No silences detected longer than the threshold. No splits will be made.");
        return Ok(());
    }

    println!("Status: Detected {} potential silence regions. Identifying final split points...", silences.len());

    // --- 5. Determine effective split points based on the user's threshold ---
    let mut split_points: Vec<f64> = Vec::new();

    // Iterate through the detected silences and add those meeting the threshold
    // as potential split points (the end of the silence marks the split).
    for silence in silences {
        if silence.duration >= silence_threshold_seconds {
            split_points.push(silence.end);
        }
    }

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
    if split_points.is_empty() || split_points.last().map_or(false, |&last_split| last_split < total_duration - 0.01) { // Add a small epsilon for float comparison
        split_points.push(total_duration);
    }

    println!("Status: Identified {} audio segments to be split.", split_points.len());

    // --- 6. Split audio using FFmpeg for each determined segment ---
    let mut current_segment_start_time = 0.0; // Start time for the first segment.

    // Get the file extension of the original audio for the output files.
    let output_file_extension = input_audio_path
        .extension()
        .unwrap_or_default() // If no extension, use empty string
        .to_string_lossy()
        .to_string();

    for (i, &split_end_time) in split_points.iter().enumerate() {
        // Calculate the duration of the current segment.
        let duration = split_end_time - current_segment_start_time;

        // Skip if the duration is non-positive (e.g., due to consecutive split points too close).
        if duration <= 0.01 { // Use a small epsilon for float comparison
            current_segment_start_time = split_end_time;
            continue;
        }

        // Construct the output file name (e.g., output/part_001.mp3).
        let output_file_name = format!("{}_{:03}.{}", output_prefix, i + 1, output_file_extension);

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
            .arg("-y") // Overwrite output files without asking
            .arg(&output_file_name)
            .status() // Get the exit status of the command.
            .map_err(|e| format!("Failed to execute ffmpeg for splitting. Error: {}", e))?;

        // Check if the splitting command was successful.
        if !status.success() {
            return Err(format!("FFmpeg splitting failed for part {}. Status: {}", i + 1, status));
        }

        // Update the start time for the next segment.
        current_segment_start_time = split_end_time;
    }

    Ok(()) // Indicate success.
}

