# Rust Audio Splitter

---

A command-line application written in Rust that intelligently splits long audio files (like MP3s) into smaller segments. It achieves this by scanning the audio for periods of silence that exceed a user-defined threshold and then splitting the file at the midpoint of these detected silences, preventing abrupt audio clipping.

## Features

* **Silence Detection:** Accurately identifies sections of silence within audio files.

* **Customizable Thresholds:** Users can specify the minimum **silence duration** and **noise threshold (in dB)** to fine-tune detection sensitivity.

* **Interactive Re-analysis:** After initial silence detection, users can choose to re-analyze the audio with different settings or proceed directly to splitting.

* **Mid-Silence Splitting:** Splits audio precisely in the middle of a detected silence to ensure smooth transitions between segments.

* **Status Updates:** Provides regular progress updates during the detection and splitting process.

* **Multi-file Processing:** Option to process multiple audio files consecutively.

## Prerequisites

Before running this application, you need to have the following installed on your system:

* **Rust:** The Rust programming language and its package manager, Cargo.

    * Install Rust: Follow the instructions on the official Rust website: <https://www.rust-lang.org/tools/install>

* **FFmpeg and FFprobe:** These are essential multimedia frameworks that the application uses for audio analysis and splitting. They must be installed and accessible in your system's PATH.

    ### FFmpeg/FFprobe Installation (Windows Guide)

    1.  **Download Binaries:**

        * Visit <https://www.gyan.dev/ffmpeg/builds/> (recommended source for Windows builds).

        * Under "release builds," download either `ffmpeg-release-essentials.7z` (most common) or `ffmpeg-release-full.7z` (includes more codecs).

    2.  **Extract FFmpeg:**

        * Locate the downloaded `.7z` or `.zip` file.

        * Use a tool like 7-Zip (or Windows' built-in extractor for `.zip`) to extract the contents.

        * **Rename the extracted folder to `FFmpeg`** for simplicity.

    3.  **Move the FFmpeg Folder:**

        * Cut the `FFmpeg` folder and paste it directly into the root of your `C:` drive (e.g., `C:\FFmpeg`).

    4.  **Add to System PATH:**

        * Open the Windows search bar (Windows Key + S), type "environment variables," and select "Edit the system environment variables."

        * In the "System Properties" window, click "Environment Variables...".

        * Under "System variables" (bottom half), find and select the `Path` variable, then click "Edit...".

        * Click "New" and add the path: `C:\FFmpeg\bin`.

        * Click "OK" on all open windows to save the changes.

    5.  **Verify Installation:**

        * Open a **new** Command Prompt or PowerShell window (changes won't apply to existing ones).

        * Type `ffmpeg -version` and press Enter.

        * Type `ffprobe -version` and press Enter.

        * If version information is displayed for both, the installation was successful.

## Installation (Application)

1.  **Clone the Repository (if applicable) or create a new project:**

    ```
    git clone <repository_url> # If this were a real repo
    cd audio_splitter
    ```

    If you're starting from scratch:

    ```
    cargo new audio_splitter
    cd audio_splitter
    ```

2.  **Add Dependencies:**
    Open `Cargo.toml` in your project root and add the `regex` dependency:

    ```toml
    # Cargo.toml
    [dependencies]
    regex = "1"
    ```

3.  **Place the Source Code:**
    Replace the content of `src/main.rs` with the Rust code provided in the `rust-audio-splitter` Canvas.

## Usage

1.  **Compile and Run the Application:**
    Navigate to your project's root directory in your terminal or Command Prompt and run:

    ```bash
    cargo run
    ```

2.  **Follow the Prompts:**
    The application will guide you through the process:

    * **Audio File Path:** Enter the full path to the audio file you want to split (e.g., `C:\Users\YourName\Music\long_podcast.mp3` or `audio.wav`).

    * **Minimum Silence Length:** Enter the minimum duration in seconds that a silence must be to be considered a split point (e.g., `0.5`, `2.0`).

    * **Noise Threshold (dB):** Enter the noise threshold in decibels (e.g., `-40.0`). A less negative value (e.g., `-30.0` or `-20.0`) will make FFmpeg more lenient, considering quieter sounds as part of a silence. A good starting point is often **-40.0dB**.

    * **Review and Act:** After detection, the application will tell you how many audio segments were identified. You will then be prompted to:

        * `(r)` **Re-analyze:** If the number of segments isn't what you expected, choose `r` to try different silence and noise threshold settings.

        * `(p)` **Proceed:** If you're satisfied with the detected split points, choose `p` to proceed with splitting the audio file.

    * **Output Prefix:** Enter the desired path and file name prefix for the split audio files (e.g., `output/part`). If the directory doesn't exist, you'll be asked if you want to create it. The application will append a sequential number and the original file's extension (e.g., `output/part_001.mp3`, `output/part_002.mp3`).

3.  **Completion:**
    The application will provide status updates as it splits the file. Once finished, it will confirm completion and ask if you wish to process another file.

## Troubleshooting and Tips

* **"No silences detected":** If you're getting this message even with seemingly quiet audio, try adjusting the **noise threshold (n value)**. Experiment with less negative values like `-30.0dB`, `-20.0dB`, or even `-10.0dB`. The optimal value depends on the specific audio's background noise.

* **"FFmpeg/FFprobe not found":** Ensure FFmpeg and FFprobe are correctly installed and their `bin` directory is added to your system's PATH environment variable. Remember to open a *new* terminal window after modifying the PATH.

* **Abrupt Splitting:** The application is designed to split in the middle of silence, but for very short silences or specific audio content, you might still perceive a subtle cut. Adjusting the `silence length threshold` can help.