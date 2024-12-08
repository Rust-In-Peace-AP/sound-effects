use rodio::{Decoder, OutputStream, Source};
use reqwest::blocking::get;
use std::io::Cursor;

fn main() {
    // Errore nel caso in qui il suono non si riesca a riprodurre
    if let Err(e) = play_sound_from_url("https://raw.githubusercontent.com/Rust-In-Peace-AP/sound-effects/main/categories/success/quackable/quack1.mp3") {
            eprintln!("Errore nella riproduzione audio: {}", e);
    }
}


fn play_sound_from_url(url: &str) -> Result<(), Box<dyn std::error::Error>> {
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
    println!("QUACK!");
    std::thread::sleep(std::time::Duration::from_secs(2)); // Aspetta 2 sec che il suono termini
    Ok(())
}
