use rodio::{Decoder, OutputStream, Source};
use std::io::{Cursor, Read};
use reqwest::blocking::get;

pub const SOUND_SENT: &str = "https://raw.githubusercontent.com/Rust-In-Peace-AP/sound-effects/main/categories/success/quackable/quack1.mp3";
pub const SOUND_RECEIVED: &str = "";
pub const SOUND_CRASH: &str = "";

pub fn play_sound_from_url(url: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Scarica il file audio
    let response = get(url)?;
    if !response.status().is_success() {
        return Err(format!("Impossibile scaricare l'audio, codice HTTP: {}", response.status()).into());
    }

    let audio_data = Cursor::new(response.bytes()?);

    // Crea uno stream audio
    let (_stream, stream_handle) = OutputStream::try_default()?;
    let source = Decoder::new(audio_data)?;
    stream_handle.play_raw(source.convert_samples())?;
    std::thread::sleep(std::time::Duration::from_secs(1)); // Aspetta 2 sec che il suono termini
    Ok(())
}