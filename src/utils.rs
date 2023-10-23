pub fn decode_bytes(bytes: &[u8]) -> Vec<i16> {
    let mut decoder = minimp3::Decoder::new(bytes);
    let mut samples = Vec::new();
    loop {
        match decoder.next_frame() {
            Ok(frame) => {
                samples.extend_from_slice(&frame.data);
            }
            Err(minimp3::Error::Eof) => break,
            Err(e) => panic!("Error decoding mp3: {:?}", e),
        }
    }
    
    samples
}
