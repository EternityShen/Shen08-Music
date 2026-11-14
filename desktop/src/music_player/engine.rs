use audio_lib::Player;
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::Duration;

pub enum Event {
    PlayUrl(String),
    PlayFile(String),
    Play,
    Pause,
    Seek(f64),
    Next,
    Prev,
}

pub struct MusicEngine {
    player: Arc<Player>,
    event_rx: Receiver<Event>,
}

impl MusicEngine {
    pub fn new(event_rx: Receiver<Event>) -> Self {
        let player = Player::new().expect("无法创建音频句柄");
        Self { player, event_rx }
    }

    fn play_url(&mut self, url: &str) {
        let _result = self.player.play_url(url);
    }

    fn play_file(&mut self, file: &str) {
        let _result = self.player.play_file(file);
    }

    fn play(&mut self) {
        let _result = self.player.play();
    }

    fn pause(&mut self) {
        let _result = self.player.pause();
    }

    fn seek(&mut self, seek_time: f64) {
        let _result = self.player.seek_time(Duration::from_secs_f64(seek_time));
    }

    fn next(&mut self) {}

    fn prev(&mut self) {}

    pub fn start_engine(&mut self) {
        loop {
            if let Ok(event) = self.event_rx.recv() {
                match event {
                    Event::PlayUrl(url) => self.play_url(&url),
                    Event::PlayFile(path) => self.play_file(&path),
                    Event::Play => self.play(),
                    Event::Pause => self.pause(),
                    Event::Seek(time) => self.seek(time),
                    Event::Next => self.next(),
                    Event::Prev => self.prev(),
                }
            }
        }
    }
}
