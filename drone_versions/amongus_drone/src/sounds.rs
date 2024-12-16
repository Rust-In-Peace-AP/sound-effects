use rodio::{Decoder, OutputStream, Source}; // Importing necessary components from the rodio library for audio playback
use std::io::{Cursor, Read}; // Importing Cursor for wrapping in-memory data and Read for handling IO operations
use reqwest::blocking::get; // Importing the blocking version of the `get` function from the reqwest library for HTTP requests

// URLs for the various Windows sound effects stored in the GitHub repository
pub const SOUND_SENT: &str = "https://raw.githubusercontent.com/Rust-In-Peace-AP/sound-effects/main/sounds/amongus_sounds/amongus_sent.mp3";
pub const SOUND_RECEIVED: &str = "https://raw.githubusercontent.com/Rust-In-Peace-AP/sound-effects/main/sounds/amongus_sounds/amongus_received.mp3";
pub const SOUND_CRASH: &str = "https://raw.githubusercontent.com/Rust-In-Peace-AP/sound-effects/main/sounds/amongus_sounds/amongus_crash.mp3";
pub const SOUND_DROPPED: &str = "https://raw.githubusercontent.com/Rust-In-Peace-AP/sound-effects/main/sounds/amongus_sounds/amongus_dropped.mp3";

// Function to play a sound from a URL
pub fn play_sound_from_url(url: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Send an HTTP GET request to the provided URL to download the audio file
    let response = get(url)?;

    // Check if the HTTP response status indicates success
    if !response.status().is_success() {
        return Err(format!("Failed to download audio, HTTP status code: {}", response.status()).into());
    }

    // Load the audio data into a Cursor for in-memory operations
    let audio_data = Cursor::new(response.bytes()?);

    // Create an audio output stream for playback.
    let (_stream, stream_handle) = OutputStream::try_default()?;

    // Decode the audio data into a source format playable by the rodio library
    let source = Decoder::new(audio_data)?;

    // Play the decoded audio on the output stream
    stream_handle.play_raw(source.convert_samples())?;

    // Wait for 3 seconds to allow the sound to finish playing
    std::thread::sleep(std::time::Duration::from_secs(3));

    // Indicate successful completion
    Ok(())
}
