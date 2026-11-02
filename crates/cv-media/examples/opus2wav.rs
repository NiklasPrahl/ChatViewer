//! Converts an Ogg Opus file to WAV on stdout (development helper).

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: opus2wav <file.opus> > out.wav");
    let data = std::fs::read(path).expect("readable file");
    let wav = cv_media::opus_to_wav(&data).expect("valid Ogg Opus");
    std::io::Write::write_all(&mut std::io::stdout(), &wav).expect("stdout");
}
