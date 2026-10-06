//! The tab's side: what the page calls (`page/main.js`).
//!
//! The page plays the Pocket3D title card and opens the shell on a canvas
//! ([`Requiem::open`]); the pack is read beside the frames ([`Requiem::read`]).
//! A frame is one call: the pad of the handheld the page shows, the ticks,
//! the scene. Everything a frame does is in `app`.

use pocket_web_wgpu::gpu::{Gpu, Screen};
use wasm_bindgen::prelude::*;

use crate::app::{App, Held, Shape, SHAPES};

fn describe(s: &Shape) -> String {
    format!("{{\"name\":\"{}\",\"width\":{},\"height\":{},\"samples\":{},\"hz\":{},\"post\":{}}}", s.name, s.width, s.height, s.samples, s.hz, s.post)
}

/// The screens the page can ask for, as a JSON array.
#[wasm_bindgen]
pub fn shapes() -> String {
    format!("[{}]", SHAPES.iter().map(describe).collect::<Vec<_>>().join(","))
}

#[wasm_bindgen]
pub struct Requiem {
    app: App,
}

#[wasm_bindgen]
impl Requiem {
    /// The shell on `canvas`, which has the shape's size in pixels, with no pack yet. One to a page.
    pub async fn open(canvas: web_sys::HtmlCanvasElement, shape: String) -> Result<Requiem, JsError> {
        std::panic::set_hook(Box::new(|info| web_sys::console::error_1(&info.to_string().into())));
        let shape = Shape::named(&shape).ok_or_else(|| JsError::new("no such shape"))?;
        let (gpu, surface) = Gpu::for_canvas(canvas).await.map_err(|e| JsError::new(&e))?;
        let screen = Screen::canvas(&gpu, surface, shape.width, shape.height, 1);
        Ok(Requiem { app: App::open(gpu, screen, shape) })
    }

    /// Starts reading the pack at `url`: the pack's file, on a server that answers byte ranges, or the
    /// manifest (`.json`) of a pack cut into pieces (`pocket_web_wgpu::source`).
    pub fn read(&mut self, url: &str) {
        self.app.read(url);
    }

    /// The field from above for a second screen: the bytes of the 3DS pack's `MAPT` section.
    pub fn map(&mut self, section: &[u8]) -> Result<(), JsError> {
        self.app.map(section).map_err(|e| JsError::new(&e))
    }

    /// Whether the game runs: its pack has arrived and is on the GPU.
    pub fn runs(&self) -> bool {
        self.app.runs()
    }

    /// One frame at `now` (the frame loop's clock, milliseconds). `buttons`: PocketJS's bits of the buttons
    /// held on the page's handheld; the sticks in -1…1, right and up positive.
    pub fn frame(&mut self, now: f64, buttons: u32, lx: f32, ly: f32, rx: f32, ry: f32) -> Result<(), JsError> {
        self.app.frame(now, &Held { buttons, left: [lx, ly], right: [rx, ry] }).map_err(|e| {
            self.app.trouble = e.clone();
            JsError::new(&e)
        })
    }

    /// Another screen from the next frame on. The canvas has the new size already. `name` is one of
    /// `shapes()`; a number that is not zero replaces that shape's.
    pub fn reshape(&mut self, name: &str, width: u32, height: u32, samples: u32, hz: u32) -> Result<String, JsError> {
        let mut shape = Shape::named(name).ok_or_else(|| JsError::new("no such shape"))?;
        let or = |value: u32, fallback: u32| if value != 0 { value } else { fallback };
        (shape.width, shape.height, shape.hz) = (or(width, shape.width), or(height, shape.height), or(hz, shape.hz));
        // (the scene's samples are the programs': they stay as the page opened with)
        shape.samples = or(samples, self.app.shape.samples);
        self.app.reshape(shape);
        Ok(describe(&self.app.shape))
    }

    /// `frames` frames of sound at `rate` a second, left and right in turn, in -1…1.
    pub fn sound(&mut self, frames: usize, rate: f32) -> Vec<f32> {
        self.app.sound(frames, rate).into_iter().map(|s| s as f32 / 32768.0).collect()
    }

    /// The second screen's map with its marks, as rows of RGBA; empty when no map was handed in.
    pub fn lower(&self) -> Vec<u8> {
        self.app.lower().0
    }

    /// The fight in numbers for the second screen, as a JSON object.
    pub fn numbers(&self) -> String {
        match self.app.numbers() {
            Some((hp, mana, kos, goal, standing, auto, won)) => format!("{{\"hp\":{hp:.3},\"mana\":{mana:.3},\"kos\":{kos},\"goal\":{goal},\"standing\":{standing},\"auto\":{auto},\"won\":{won}}}"),
            None => "null".into(),
        }
    }

    /// Words for the run, as a development host sends them (`App::control`).
    pub fn control(&mut self, words: &str) {
        self.app.control(words);
    }

    /// The run as a JSON object.
    pub fn status(&self) -> String {
        self.app.status()
    }
}
