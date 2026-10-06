//! Frames of the game written to a file, on the build machine: the renderer
//! of the browser tab on this machine's GPU, reading the pack from disk.
//!
//!   requiem-shot --pack stage.pack --out frame.png [--shape vita] [--size 960x544] [--frames 240]
//!                [--words "auto=1 view=… cast=…"] [--at 300:"view=…" …] [--status status.json]
//!   requiem-shot --pack stage.pack --film - --from 120 --frames 600 [--out last.png]
//!
//! `--pack` names the pack's file, or the manifest (`.json`) of a pack cut into
//! pieces. The run makes `--frames` frames, each a thirtieth of a second (two
//! ticks) at the shape's rate, with nothing held on the pad: the autopilot
//! plays unless `--words` say otherwise. `--at N:"words"` hands words in before
//! frame N, any number of times. The last frame is the picture.
//!
//! `--film PATH` (or `-` for the standard output) writes every frame from
//! `--from` on as rows of RGBA, one after another, for an encoder to read.

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    if let Err(e) = native::run() {
        eprintln!("requiem-shot: {e}");
        std::process::exit(1);
    }
}

#[cfg(target_arch = "wasm32")]
fn main() {}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::io::Write;

    use pocket_web_wgpu::gpu::{Gpu, Screen};
    use pocket_web_wgpu::task;
    use requiem_wgpu::app::{App, Held, Shape};

    fn options(name: &str) -> Vec<String> {
        let args: Vec<String> = std::env::args().collect();
        args.iter().enumerate().filter(|(_, a)| *a == name).filter_map(|(i, _)| args.get(i + 1).cloned()).collect()
    }

    fn option(name: &str) -> Option<String> {
        options(name).into_iter().next()
    }

    pub fn run() -> Result<(), String> {
        let pack = option("--pack").ok_or("--pack PATH")?;
        let mut shape = Shape::named(&option("--shape").unwrap_or("vita".into())).ok_or("--shape vita | psp | 3ds")?;
        if let Some(size) = option("--size") {
            let (w, h) = size.split_once('x').ok_or("--size WIDTHxHEIGHT")?;
            (shape.width, shape.height) = (w.parse().map_err(|_| "--size WIDTHxHEIGHT")?, h.parse().map_err(|_| "--size WIDTHxHEIGHT")?);
        }
        let number = |name: &str, fallback: u32| option(name).map_or(Ok(fallback), |v| v.parse::<u32>().map_err(|_| format!("{name} takes a number")));
        shape.samples = number("--samples", shape.samples)?;
        shape.hz = number("--hz", shape.hz)?;
        let frames = number("--frames", 240)?;
        let from = number("--from", 0)?;
        let mut at: Vec<(u32, String)> = options("--at").iter().filter_map(|a| a.split_once(':').and_then(|(n, words)| Some((n.parse().ok()?, words.to_string())))).collect();
        at.sort_by_key(|a| a.0);

        let gpu = task::wait(Gpu::headless())?;
        let screen = Screen::texture(&gpu, shape.width, shape.height, 1);
        let mut app = App::open(gpu, screen, shape);
        app.read(&pack);
        if let Some(map) = option("--map") {
            app.map(&std::fs::read(&map).map_err(|e| format!("{map}: {e}"))?)?;
        }
        if let Some(words) = option("--words") {
            app.control(&words);
        }
        let mut film: Option<Box<dyn Write>> = match option("--film").as_deref() {
            Some("-") => Some(Box::new(std::io::BufWriter::new(std::io::stdout().lock()))),
            Some(path) => Some(Box::new(std::io::BufWriter::new(std::fs::File::create(path).map_err(|e| format!("{path}: {e}"))?))),
            None => None,
        };
        let pad = Held::default();
        let step = 1000.0 / shape.hz as f64;
        // (the first frame hands the pack to the GPU and draws nothing of the game)
        app.frame(0.0, &pad)?;
        if !app.runs() {
            return Err(if app.trouble.is_empty() { "the pack did not arrive".into() } else { app.trouble.clone() });
        }
        let mut pixels = Vec::new();
        for count in 0..frames.max(1) {
            for (_, words) in at.iter().filter(|a| a.0 == count) {
                app.control(words);
            }
            app.frame((count + 1) as f64 * step, &pad)?;
            if count + 1 == frames.max(1) || (film.is_some() && count >= from) {
                pixels = task::wait(app.screen.read(&app.gpu))?;
            }
            if let (Some(out), true) = (&mut film, count >= from) {
                out.write_all(&pixels).map_err(|e| format!("the film: {e}"))?;
            }
        }
        if let Some(out) = &mut film {
            out.flush().map_err(|e| format!("the film: {e}"))?;
        }
        if let Some(out) = option("--out") {
            let file = std::fs::File::create(&out).map_err(|e| format!("{out}: {e}"))?;
            let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), shape.width, shape.height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.write_header().and_then(|mut w| w.write_image_data(&pixels)).map_err(|e| format!("{out}: {e}"))?;
        }
        if let Some(lower) = option("--lower") {
            let (map, size) = app.lower();
            if size > 0 {
                let file = std::fs::File::create(&lower).map_err(|e| format!("{lower}: {e}"))?;
                let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), size as u32, size as u32);
                encoder.set_color(png::ColorType::Rgba);
                encoder.set_depth(png::BitDepth::Eight);
                encoder.write_header().and_then(|mut w| w.write_image_data(&map)).map_err(|e| format!("{lower}: {e}"))?;
            }
        }
        let status = app.status();
        if let Some(path) = option("--status") {
            std::fs::write(&path, &status).map_err(|e| format!("{path}: {e}"))?;
        }
        // (the film may be on the standard output: the status goes beside it)
        eprintln!("{status}");
        Ok(())
    }
}
