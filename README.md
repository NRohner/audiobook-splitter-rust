# Rust Audio Splitter

---

A Rust application (local web GUI plus an interactive CLI) that intelligently splits long audio files (MP3, M4B, WAV, FLAC, AAC, M4A or OGG) into smaller segments. It achieves this by scanning the audio for periods of silence that exceed a user-defined threshold and then splitting the file at the midpoint of these detected silences, preventing abrupt audio clipping.

## Features

* **Silence Detection:** Accurately identifies sections of silence within audio files.

* **Customizable Thresholds:** Users can specify the minimum **silence duration** and **noise threshold (in dB)** to fine-tune detection sensitivity.

* **Interactive Re-analysis:** After initial silence detection, users can choose to re-analyze the audio with different settings or proceed directly to splitting.

* **Mid-Silence Splitting:** Splits audio precisely in the middle of a detected silence to ensure smooth transitions between segments.

* **Status Updates:** Provides regular progress updates during the detection and splitting process.

* **AI Chapter Matching (optional):** Give it the book's EPUB and a local Whisper speech-to-text model transcribes ~20 seconds either side of each silence. The app splits only where the narration matches the start of an EPUB chapter, and names the files after the chapters (e.g. `03 - The Storm.mp3`). In the GUI you can play 30 seconds from each matched break to check it by ear. Everything runs offline on your machine.

* **Chapter Editor (GUI):** After analysis, rename, remove or add chapters, move a break to the previous or next silence, add one at any time (`h:mm:ss`), or promote a silence the analysis skipped. Every break can be played back to check it by ear.

* **Metadata Editor (GUI):** Set the album, artist, album artist, genre and year for the whole book, and a track naming convention such as `{n} - {title}` (`{n}` counts from 00; `{album}` is also available). Every output file is tagged disc 1 with sequential track numbers across the whole book. Empty fields keep the source file's tags.

* **Multi-file Processing:** Option to process multiple audio files consecutively.

## Setup

You need four things on any platform:

* **Rust** (with Cargo): <https://www.rust-lang.org/tools/install>
* **CMake** and a **C/C++ compiler**: the Whisper speech-to-text engine (whisper.cpp) is compiled into the app.
* **FFmpeg and FFprobe** on your `PATH`: the app uses them to analyze and split audio.
* The **Whisper model** (`models/ggml-base.en.bin`, ~148 MB): downloaded once, see below.

Windows also needs the **Vulkan SDK**, so Whisper can run on the GPU.

### macOS

1.  **Install the Xcode command line tools** (C/C++ compiler and `make`):

    ```bash
    xcode-select --install
    ```

2.  **Install Rust:**

    ```bash
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
    ```

3.  **Install CMake and FFmpeg** with [Homebrew](https://brew.sh) (FFprobe ships with FFmpeg):

    ```bash
    brew install cmake ffmpeg
    ```

4.  **Clone and run:**

    ```bash
    git clone https://github.com/NRohner/audiobook-splitter.git
    cd audiobook-splitter
    make run     # downloads the Whisper model on first run, builds the release binary, opens the GUI
    ```

On Apple Silicon, Whisper runs on the GPU via Metal.

### Windows

1.  **Install Rust:** download and run `rustup-init.exe` from <https://www.rust-lang.org/tools/install>. When asked, let it install the **Visual Studio C++ Build Tools** (the "Desktop development with C++" workload); that is the C/C++ compiler.

2.  **Install CMake, Git, LLVM and the Vulkan SDK** (LLVM provides `libclang.dll`, which the Whisper bindings need at build time; the Vulkan SDK lets Whisper run on any NVIDIA, AMD or Intel GPU):

    ```powershell
    winget install Kitware.CMake Git.Git LLVM.LLVM KhronosGroup.VulkanSDK
    [Environment]::SetEnvironmentVariable("LIBCLANG_PATH", "C:\Program Files\LLVM\bin", "User")
    [Environment]::SetEnvironmentVariable("CARGO_TARGET_DIR", "C:\abs-target", "User")
    ```

    The Vulkan SDK installer sets `VULKAN_SDK` for you. `CARGO_TARGET_DIR` moves the build folder to a short path: whisper.cpp's GPU code builds files nested so deep that, under the repo's `target\` folder, they pass Windows' 260-character path limit and the build fails. The built binary is then `C:\abs-target\release\audiobook-splitter-rust.exe`; run it (or `cargo run`) from the repo folder so it finds `models\`.

    Nothing GPU-specific is needed to *run* the app: your graphics driver already includes Vulkan. Without a usable GPU, Whisper falls back to the CPU (roughly 50 times slower).

3.  **Install FFmpeg and FFprobe.** The quickest way is winget, which also adds them to your `PATH`:

    ```powershell
    winget install Gyan.FFmpeg
    ```

    <details>
    <summary>Manual install instead</summary>

    1.  Download `ffmpeg-release-essentials.7z` (or `ffmpeg-release-full.7z` for more codecs) from <https://www.gyan.dev/ffmpeg/builds/>.
    2.  Extract it with 7-Zip, rename the extracted folder to `FFmpeg` and move it to `C:\FFmpeg`.
    3.  Search the Start menu for "environment variables", open **Edit the system environment variables**, click **Environment Variables...**, select `Path` under **System variables**, click **Edit...**, then **New**, and add `C:\FFmpeg\bin`.
    4.  Click **OK** on every window.

    </details>

4.  **Check the tools:** open a **new** PowerShell window (`PATH` changes don't reach windows that were already open) and run:

    ```powershell
    cargo --version
    cmake --version
    Test-Path "$env:LIBCLANG_PATH\libclang.dll"   # should print True
    Test-Path "$env:VULKAN_SDK\Lib\vulkan-1.lib"  # should print True
    ffmpeg -version
    ffprobe -version
    ```

5.  **Clone, download the model and run.** The Makefile needs `make`, which Windows doesn't include, so run the equivalent commands directly:

    ```powershell
    git clone https://github.com/NRohner/audiobook-splitter.git
    cd audiobook-splitter
    mkdir models
    curl.exe -L --fail -o models\ggml-base.en.bin https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin
    cargo run --release   # builds the release binary, opens the GUI
    ```

    Use `curl.exe`, not `curl`: in Windows PowerShell 5, `curl` is an alias for `Invoke-WebRequest`, which takes different arguments.

## Running

| macOS | Windows (PowerShell) | What it does |
|---|---|---|
| `make model` | see step 5 above | One-time: download the Whisper model into `models/` |
| `make dev` | `cargo run` | Debug build, starts the GUI and opens your browser |
| `make build` | `cargo build --release` | Optimized single binary at `target/release/audiobook-splitter-rust` (`C:\abs-target\release\audiobook-splitter-rust.exe` on Windows) |
| `make run` | `cargo run --release` | Build and launch the release GUI |
| `make test` | `cargo test` | Run the test suite |
| `make cli` | `cargo run -- --cli` | Original interactive terminal flow |

The GUI is a local web page served by the app itself at `http://127.0.0.1:7878` (it only listens on your machine). Pick an audio file or folder and an output folder (type a path or use the native **File** / **Folder** / **Browse** dialogs), set the detection thresholds, click **Analyze**, tweak and re-analyze until the segment counts look right, then click **Split files**.

The binary is self-contained (the page is compiled in); FFmpeg/FFprobe still need to be on your `PATH`. Set `WHISPER_MODEL=/path/to/ggml-*.bin` to use a different model (e.g. `ggml-small.en.bin` for higher accuracy). In PowerShell: `$env:WHISPER_MODEL = "C:\path\to\ggml-small.en.bin"`.

### Matching chapters with an EPUB

1. Fill in **Book EPUB** (the same edition as the audiobook works best; its table of contents should mark the chapters).
2. Use a *looser* minimum silence than usual (e.g. 1.0-2.0 s): silences are only candidates now, and the transcript match decides which are real chapter breaks.
3. Click **Analyze**. The results list each chapter break with a score (0-2: how well the speech after the pause matches the chapter's opening, plus how well the speech before it matches the previous chapter's ending) and what Whisper heard. Chapters with no match are listed; front matter like the copyright page usually isn't narrated, so that's expected.
4. If real chapters are missing, lower the minimum silence or the **Chapter match threshold** (default 0.4) and analyze again.
5. **Split files** writes one file per chapter. A chapter that crosses an audio file boundary becomes `NN - Title (part 2)`, and anything before the first chapter is `00 - Opening`. Run it with `--cli` for the terminal version.

## Usage (CLI)

1.  **Compile and Run the Application:**
    In the project's root directory, run:

    ```bash
    make cli             # macOS
    cargo run -- --cli   # Windows
    ```

2.  **Follow the Prompts:**
    The application will guide you through the process:

    * **Audio File Path:** Enter the full path to the audio file you want to split (e.g., `~/Music/long_podcast.mp3` on macOS, `C:\Users\YourName\Music\long_podcast.mp3` on Windows, or `audio.wav`).

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

* **"FFmpeg/FFprobe not found":** Ensure FFmpeg and FFprobe are correctly installed and their `bin` directory is added to your system's PATH environment variable. On macOS, `brew install ffmpeg` handles this. On Windows, remember to open a *new* terminal window after modifying the PATH.

* **Abrupt Splitting:** The application is designed to split in the middle of silence, but for very short silences or specific audio content, you might still perceive a subtle cut. Adjusting the `silence length threshold` can help.