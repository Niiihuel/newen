//! The clip of a release's announcement (quill-ui's whats_new.rs): Cisco's codec loaded once the
//! engine has it and the person allows it, the clip read and parsed, both off the window's
//! thread; the player made when the card opens and dropped when it closes. A codec that does
//! not load (on NixOS, without libstdc++ for Cisco's library) leaves the poster, said in the log.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use eframe::egui;
use limn_video::{Clip, Codec, Status, VideoPlayer};
use newen_model::CodecState;
use newen_ui::Ui;

#[derive(Default)]
pub struct Clips {
    codec_asked: Option<PathBuf>,
    codec_done: Arc<Mutex<Option<Codec>>>,
    clip: Option<(PathBuf, Clip)>,
    clip_asked: Option<PathBuf>,
    clip_done: Arc<Mutex<Option<(PathBuf, Clip)>>>,
}

impl Clips {
    /// Before the view is built: what the card needs, asked for or taken.
    pub fn tick(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        let ready = match (&ui.codec, ui.settings.video_codec) {
            (CodecState::Ready(path), Some(true)) => Some(path.clone()),
            _ => None,
        };
        match ready {
            // Turned off: the codec goes, and with it the player that used it.
            None => {
                if ui.codec_loaded.take().is_some() {
                    ui.video = None;
                }
                self.codec_asked = None;
            }
            Some(path) if self.codec_asked.as_ref() != Some(&path) => {
                self.codec_asked = Some(path.clone());
                let (done, ctx) = (self.codec_done.clone(), ctx.clone());
                std::thread::spawn(move || match Codec::load(&path) {
                    Ok(codec) => {
                        *done.lock().unwrap() = Some(codec);
                        ctx.request_repaint();
                    }
                    Err(error) => eprintln!(
                        "quill: video codec {} does not load ({error}): clips show their poster",
                        path.display()
                    ),
                });
            }
            Some(_) => {}
        }
        if let Some(codec) = self.codec_done.lock().unwrap().take() {
            ui.codec_loaded = Some(codec);
        }
        let Some(path) = newen_ui::whats_new::clip(ui) else {
            ui.video = None;
            self.clip = None;
            self.clip_asked = None;
            return;
        };
        if self.clip.as_ref().map(|(p, _)| p) != Some(&path)
            && self.clip_asked.as_ref() != Some(&path)
        {
            self.clip_asked = Some(path.clone());
            let (done, ctx) = (self.clip_done.clone(), ctx.clone());
            std::thread::spawn(move || {
                let clip = std::fs::read(&path)
                    .map_err(|error| error.to_string())
                    .and_then(|bytes| Clip::from_mp4(&bytes).map_err(|error| error.to_string()));
                match clip {
                    Ok(clip) => {
                        *done.lock().unwrap() = Some((path, clip));
                        ctx.request_repaint();
                    }
                    Err(error) => eprintln!(
                        "quill: what's new: clip {} not played ({error}): its poster shows",
                        path.display()
                    ),
                }
            });
        }
        if let Some(clip) = self.clip_done.lock().unwrap().take() {
            self.clip = Some(clip);
            ui.video = None;
        }
        let Some((_, clip)) = &self.clip else {
            return;
        };
        let codec = ui.codec_loaded.clone();
        let stale = ui
            .video
            .as_ref()
            .is_none_or(|player| player.status() == Status::NoCodec && codec.is_some());
        if stale {
            // Muted and looped; under reduced motion it waits on its poster.
            ui.video = Some(VideoPlayer::new(
                newen_ui::whats_new::VIDEO,
                clip.clone(),
                !ui.reduced_motion,
                codec,
            ));
        }
    }
}
