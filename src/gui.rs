// Local web GUI: a tiny HTTP server on 127.0.0.1 serving one embedded page.
// Requests are handled on the main thread (macOS requires native file dialogs there);
// analysis and splitting run on a background thread and the page polls /status.
use audiobook_splitter_rust::{collect_inputs, detect_silences, split_file, split_points, AUDIO_EXTENSIONS};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;
use tiny_http::{Header, Method, Request, Response, Server};

const PAGE: &str = include_str!("ui.html");
const DEFAULT_PORT: u16 = 7878;

#[derive(Serialize, Clone)]
struct FileRow {
    name: String,
    segments: Option<usize>,
    error: Option<String>,
}

#[derive(Serialize, Default)]
struct Status {
    running: bool,
    phase: &'static str, // "analyze", "split" or "" before anything has run
    done: usize,
    total: usize,
    files: Vec<FileRow>,
    log: Vec<String>,
    #[serde(skip)]
    plan: Vec<(PathBuf, Vec<f64>)>, // Split points from the last analysis.
}

type Shared = Arc<Mutex<Status>>;

#[derive(Deserialize)]
struct AnalyzeReq {
    input: String,
    min_silence: f64,
    noise_db: f64,
}

#[derive(Deserialize)]
struct SplitReq {
    output: String,
}

#[derive(Deserialize)]
struct BrowseReq {
    kind: String, // "file" or "folder"
}

pub fn run() {
    // ponytail: fixed port so a reload keeps working; falls back to any free port.
    let server = Server::http(("127.0.0.1", DEFAULT_PORT))
        .or_else(|_| Server::http("127.0.0.1:0"))
        .expect("Failed to start the GUI server");
    let port = server.server_addr().to_ip().unwrap().port();
    let url = format!("http://127.0.0.1:{}", port);
    println!("Audio Splitter GUI running at {} (Ctrl+C to quit)", url);
    open_browser(&url);

    let state: Shared = Arc::default();
    for req in server.incoming_requests() {
        handle(req, &state, port);
    }
}

fn open_browser(url: &str) {
    let result = if cfg!(target_os = "macos") {
        Command::new("open").arg(url).spawn()
    } else if cfg!(windows) {
        Command::new("cmd").args(["/C", "start", "", url]).spawn()
    } else {
        Command::new("xdg-open").arg(url).spawn()
    };
    if result.is_err() {
        println!("Open {} in your browser.", url);
    }
}

fn handle(mut req: Request, state: &Shared, port: u16) {
    // Only answer requests addressed to us, so other websites can't drive the
    // app (DNS rebinding). POSTs must be JSON, which forces a CORS preflight
    // that we never approve, so cross-site form posts are rejected too.
    let host_ok = header(&req, "Host")
        .is_some_and(|h| h == format!("127.0.0.1:{}", port) || h == format!("localhost:{}", port));
    let json_ok = header(&req, "Content-Type").is_some_and(|c| c.starts_with("application/json"));
    if !host_ok || (*req.method() == Method::Post && !json_ok) {
        let _ = req.respond(Response::from_string("Forbidden").with_status_code(403));
        return;
    }

    let mut body = String::new();
    let _ = req.as_reader().read_to_string(&mut body);

    let result = match (req.method(), req.url()) {
        (Method::Get, "/") => {
            let html = Header::from_bytes("Content-Type", "text/html; charset=utf-8").unwrap();
            let _ = req.respond(Response::from_string(PAGE).with_header(html));
            return;
        }
        (Method::Get, "/status") => Ok(serde_json::to_value(&*state.lock().unwrap()).unwrap()),
        (Method::Post, "/browse") => parse(&body).map(browse),
        (Method::Post, "/analyze") => parse(&body).and_then(|r| start_analyze(r, state)),
        (Method::Post, "/split") => parse(&body).and_then(|r| start_split(r, state)),
        _ => {
            let _ = req.respond(Response::from_string("Not found").with_status_code(404));
            return;
        }
    };

    let (code, value) = match result {
        Ok(v) => (200, v),
        Err(e) => (400, json!({ "error": e })),
    };
    let ct = Header::from_bytes("Content-Type", "application/json").unwrap();
    let _ = req.respond(Response::from_string(value.to_string()).with_status_code(code).with_header(ct));
}

fn header(req: &Request, name: &'static str) -> Option<String> {
    req.headers()
        .iter()
        .find(|h| h.field.equiv(name))
        .map(|h| h.value.as_str().to_string())
}

fn parse<T: for<'de> Deserialize<'de>>(body: &str) -> Result<T, String> {
    serde_json::from_str(body).map_err(|e| format!("Bad request: {}", e))
}

fn browse(req: BrowseReq) -> Value {
    let dialog = rfd::FileDialog::new();
    let picked = if req.kind == "file" {
        dialog.add_filter("Audio", &AUDIO_EXTENSIONS).pick_file()
    } else {
        dialog.pick_folder()
    };
    json!({ "path": picked.map(|p| p.to_string_lossy().to_string()) })
}

fn start_analyze(req: AnalyzeReq, state: &Shared) -> Result<Value, String> {
    if !(req.min_silence > 0.0) {
        return Err("Minimum silence must be a positive number.".into());
    }
    if !req.noise_db.is_finite() {
        return Err("Noise threshold must be a number.".into());
    }
    let inputs = collect_inputs(&PathBuf::from(req.input.trim()))?;
    if inputs.is_empty() {
        return Err("No supported audio files found in that folder.".into());
    }

    {
        let mut s = state.lock().unwrap();
        if s.running {
            return Err("A job is already running.".into());
        }
        *s = Status {
            running: true,
            phase: "analyze",
            total: inputs.len(),
            files: inputs.iter().map(|p| FileRow { name: file_name(p), segments: None, error: None }).collect(),
            ..Default::default()
        };
        s.log.push(format!("Analyzing {} file(s): min silence {}s, noise {}dB", inputs.len(), req.min_silence, req.noise_db));
    }

    let state = state.clone();
    thread::spawn(move || {
        for (i, path) in inputs.iter().enumerate() {
            let result = detect_silences(path, req.min_silence, req.noise_db)
                .map(|(silences, total)| split_points(&silences, req.min_silence, total));
            let mut s = state.lock().unwrap();
            match result {
                Ok(points) => {
                    s.files[i].segments = Some(points.len());
                    s.log.push(format!("{}: {} segments", file_name(path), points.len()));
                    if !points.is_empty() {
                        s.plan.push((path.clone(), points));
                    }
                }
                Err(e) => {
                    s.files[i].error = Some(e.clone());
                    s.log.push(format!("{}: {}", file_name(path), e));
                }
            }
            s.done = i + 1;
        }
        let mut s = state.lock().unwrap();
        s.running = false;
        s.log.push("Analysis complete.".into());
    });
    Ok(json!({}))
}

fn start_split(req: SplitReq, state: &Shared) -> Result<Value, String> {
    let output = PathBuf::from(req.output.trim());
    if req.output.trim().is_empty() {
        return Err("Choose an output folder.".into());
    }
    if output.exists() && !output.is_dir() {
        return Err(format!("'{}' is not a folder.", output.display()));
    }

    let plan = {
        let mut s = state.lock().unwrap();
        if s.running {
            return Err("A job is already running.".into());
        }
        if s.phase != "analyze" || s.plan.is_empty() {
            return Err("Run an analysis that finds segments first.".into());
        }
        std::fs::create_dir_all(&output)
            .map_err(|e| format!("Failed to create '{}': {}", output.display(), e))?;
        s.running = true;
        s.phase = "split";
        s.done = 0;
        s.total = s.plan.iter().map(|(_, p)| p.len()).sum();
        s.log.push(format!("Splitting into {}", output.display()));
        std::mem::take(&mut s.plan)
    };

    let state = state.clone();
    thread::spawn(move || {
        for (path, points) in &plan {
            let result = split_file(path, &output, points, |_, _, out| {
                let mut s = state.lock().unwrap();
                s.done += 1;
                s.log.push(format!("Writing {}", file_name(out)));
            });
            let mut s = state.lock().unwrap();
            match result {
                Ok(n) => s.log.push(format!("{}: wrote {} file(s)", file_name(path), n)),
                Err(e) => s.log.push(format!("{}: {}", file_name(path), e)),
            }
        }
        let mut s = state.lock().unwrap();
        s.done = s.total;
        s.running = false;
        s.log.push("Done.".into());
    });
    Ok(json!({}))
}

fn file_name(path: &std::path::Path) -> String {
    path.file_name().unwrap_or_default().to_string_lossy().to_string()
}
