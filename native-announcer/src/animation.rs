use crate::characters::ResolvedVisual;
use crate::{chimera, video};
use std::sync::Arc;
use std::time::Duration;

pub enum Animation {
    Video(video::Video),
    Chimera(chimera::Head),
}

impl Animation {
    pub fn open(visual: &ResolvedVisual) -> Result<Self, String> {
        match visual {
            ResolvedVisual::Video(path) => video::Video::open(path).map(Self::Video),
            ResolvedVisual::Chimera if cfg!(target_os = "windows") => Ok(Self::Chimera(chimera::Head::new())),
            ResolvedVisual::Chimera => Err("CHIMERA requires Windows audio output synchronization.".into()),
        }
    }

    pub fn attach(&mut self, playback: Arc<chimera::Playback>) {
        if let Self::Chimera(head) = self { head.attach(playback); }
    }

    pub fn advance(&mut self, elapsed: Duration) -> Result<(), String> {
        match self {
            Self::Video(video) => video.advance(elapsed),
            Self::Chimera(head) => { head.advance(elapsed); Ok(()) }
        }
    }

    pub fn frame(&self) -> &image::RgbaImage {
        match self { Self::Video(video) => video.frame(), Self::Chimera(head) => head.frame() }
    }

    pub fn fps(&self) -> f64 {
        match self { Self::Video(video) => video.fps(), Self::Chimera(_) => 60.0 }
    }

    pub fn decoded_frames(&self) -> u64 {
        match self { Self::Video(video) => video.decoded_frames, Self::Chimera(_) => 0 }
    }

    pub fn loops(&self) -> u64 {
        match self { Self::Video(video) => video.loops, Self::Chimera(_) => 0 }
    }

    pub fn procedural_frames(&self) -> u64 {
        match self { Self::Video(_) => 0, Self::Chimera(head) => head.frames() }
    }

    pub fn sample_position(&self) -> u64 {
        match self { Self::Video(_) => 0, Self::Chimera(head) => head.sample_position() }
    }
}
