mod gui;

use audiobook_splitter_rust::{collect_inputs, detect_silences, split_file, split_points};
use std::io::{self, Write};
use std::path::PathBuf;

fn main() {
    if std::env::args().any(|a| a == "--cli") {
        run_cli();
    } else {
        gui::run();
    }
}

// Prints `message` and returns the trimmed line the user typed.
fn prompt(message: &str) -> String {
    print!("{}", message);
    io::stdout().flush().unwrap();
    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();
    input.trim().to_string()
}

fn run_cli() {
    println!("Welcome to the Audio Splitter!");
    println!("--------------------------------");
    println!("Note: This application requires FFmpeg and FFprobe to be installed");
    println!("      and accessible in your system's PATH for audio processing.");
    println!("--------------------------------");

    loop {
        process_selection();
        if !prompt("\nDo you want to process another file or folder? (y/n): ").eq_ignore_ascii_case("y") {
            break;
        }
    }
    println!("\nThank you for using the Audio Splitter! Goodbye.");
}

fn process_selection() {
    let input_paths = loop {
        let choice = prompt("Do you want to process a (s)ingle file or a (f)older of files? (s/f): ").to_lowercase();
        let path = match choice.as_str() {
            "s" => PathBuf::from(prompt("Enter the path to the audio file (e.g., audio.mp3 or C:\\path\\to\\audio.wav): ")),
            "f" => PathBuf::from(prompt("Enter the path to the folder containing audio files: ")),
            _ => {
                println!("Invalid choice. Please enter 's' or 'f'.");
                continue;
            }
        };
        match collect_inputs(&path) {
            Ok(paths) if paths.is_empty() => println!("No supported audio files found in the specified folder. Please try again."),
            Ok(paths) => {
                println!("Status: Found {} audio file(s).", paths.len());
                break paths;
            }
            Err(e) => println!("Error: {} Please try again.", e),
        }
    };

    let output_dir = loop {
        let dir = PathBuf::from(prompt("Enter the base output directory (e.g., output_splits or C:\\MyAudioSplits): "));
        if dir.is_dir() {
            break dir;
        } else if dir.exists() {
            println!("Error: The provided path is not a directory. Please enter a valid directory path.");
        } else if prompt(&format!("Output directory '{}' does not exist. Create it? (y/n): ", dir.display())).eq_ignore_ascii_case("y") {
            match std::fs::create_dir_all(&dir) {
                Ok(_) => break dir,
                Err(e) => eprintln!("Failed to create directory '{}': {}", dir.display(), e),
            }
        } else {
            println!("Cannot proceed without a valid output directory.");
        }
    };

    // Analysis loop: re-run with new settings until the user is happy.
    let plan = loop {
        let min_silence = loop {
            match prompt("Enter the minimum silence length in seconds (e.g., 2.0): ").parse::<f64>() {
                Ok(t) if t > 0.0 => break t,
                _ => println!("Error: Invalid threshold. Please enter a positive number."),
            }
        };
        let noise_db = loop {
            match prompt("Enter the noise threshold in dB (e.g., -40.0). Suggestion: -40.0dB. Less negative values detect more silence: ").parse::<f64>() {
                Ok(n) => break n,
                _ => println!("Error: Invalid noise threshold. Please enter a number (e.g., -40.0)."),
            }
        };

        println!("\nStatus: Detecting silences in {} file(s) with threshold {:.2}s and noise {}dB...", input_paths.len(), min_silence, noise_db);
        println!("(This might take a while for long audio files)");

        let mut plan: Vec<(PathBuf, Vec<f64>)> = Vec::new();
        for path in &input_paths {
            print!("  Analyzing '{}'...", path.display());
            io::stdout().flush().unwrap();
            match detect_silences(path, min_silence, noise_db) {
                Ok((silences, total)) => {
                    let points = split_points(&silences, min_silence, total);
                    println!(" Identified {} segments.", points.len());
                    if !points.is_empty() {
                        plan.push((path.clone(), points));
                    }
                }
                Err(e) => eprintln!("\n  Error analyzing '{}': {}\n  (This file will be skipped.)", path.display(), e),
            }
        }

        let total_segments: usize = plan.iter().map(|(_, p)| p.len()).sum();
        if total_segments == 0 {
            println!("\nNo silences detected longer than the specified threshold across all files with current settings.");
            if prompt("Do you want to re-analyze with different settings? (y/n): ").eq_ignore_ascii_case("y") {
                continue;
            }
            println!("No splitting performed for the current selection.");
            return;
        }

        println!("\nSummary: Identified {} total audio segments across {} of {} file(s).", total_segments, plan.len(), input_paths.len());
        match prompt("Do you want to (r)e-analyze with different settings or (p)roceed to split all files? (r/p): ").to_lowercase().as_str() {
            "p" => break plan,
            "r" => continue,
            _ => println!("Invalid choice. Re-analyzing by default..."),
        }
    };

    for (path, points) in &plan {
        println!("\n--- Splitting: {} ---", path.display());
        let result = split_file(path, &output_dir, points, |part, total, out| {
            println!("  Status: Writing part {}/{} to '{}'...", part, total, out.display());
        });
        match result {
            Ok(n) => println!("Successfully wrote {} file(s) for {}.", n, path.display()),
            Err(e) => eprintln!("An error occurred during splitting {}: {}", path.display(), e),
        }
    }
}
