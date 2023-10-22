use alsa::{mixer::SelemId, pcm::HwParams};
use crossbeam::channel::{unbounded, Receiver, Sender};
use std::thread::{spawn, JoinHandle};

pub enum OutputAudioTaskCommand {
    Play(Vec<i16>),
    Stop,
    SetVolume(i64),
    SetMute(bool),
    Exit,
}

// const OUTPUT_HARDWARE_NAME: &str = "plughw:1";
// const MIXER_HARDWARE_NAME: &str = "hw:1";
// const MIXER_SELEM_NAME: &str = "SoftMaster";
const OUTPUT_HARDWARE_NAME: &str = "default";
const MIXER_HARDWARE_NAME: &str = "default";
const MIXER_SELEM_NAME: &str = "Master";

fn output_audio(receiver: Receiver<OutputAudioTaskCommand>) -> anyhow::Result<()> {
    let output_pcm = alsa::PCM::new(OUTPUT_HARDWARE_NAME, alsa::Direction::Playback, true)?;

    let hw_params = HwParams::any(&output_pcm)?;
    hw_params.set_access(alsa::pcm::Access::RWInterleaved)?;
    hw_params.set_format(alsa::pcm::Format::S16LE)?;
    hw_params.set_channels(2)?;
    hw_params.set_rate(48000, alsa::ValueOr::Nearest)?;

    output_pcm.hw_params(&hw_params)?;

    let mixer = alsa::Mixer::new(MIXER_HARDWARE_NAME, false)?;

    let selem_id = SelemId::new(MIXER_SELEM_NAME, 0);

    let selem = mixer.find_selem(&selem_id).unwrap();

    selem.set_playback_volume_range(0, 100)?;
    selem.set_playback_volume_all(100)?;

    let io = output_pcm.io_i16()?;

    let mut play_buffer = Vec::<i16>::new();

    loop {
        // Check if we have any audio to play AND if we are not currently playing
        let pcm_status = output_pcm.status()?;
        if play_buffer.len() > 0 && pcm_status.get_state() != alsa::pcm::State::Running {
            // Write ONLY the amount of audio that we can fit in the buffer
            let buffer_size = pcm_status.get_avail() as usize;

            let buffer_size = if buffer_size > play_buffer.len() {
                play_buffer.len()
            } else {
                buffer_size
            };

            let buffer = play_buffer.drain(0..buffer_size).collect::<Vec<i16>>();

            output_pcm.prepare()?;
            io.writei(&buffer)?;
        }
        // Receive a command from the main thread
        if let Ok(cmd) = receiver.recv_timeout(std::time::Duration::from_millis(5)) {
            match cmd {
                OutputAudioTaskCommand::Play(buffer) => {
                    // Play doesn't actually play, it just buffers the audio
                    play_buffer.extend_from_slice(&buffer);
                }
                OutputAudioTaskCommand::Stop => {
                    // Stop playing
                    output_pcm.drop()?;
                    play_buffer.clear();
                }
                OutputAudioTaskCommand::SetMute(mute) => {
                    // Set mute
                    if mute {
                        selem.set_playback_volume_all(0)?;
                    } else {
                        selem.set_playback_volume_all(100)?;
                    }
                }
                OutputAudioTaskCommand::SetVolume(volume) => {
                    // Set volume
                    selem.set_playback_volume_all(volume)?;
                }
                OutputAudioTaskCommand::Exit => {
                    // Exit the thread
                    break;
                }
            }
        }
    }
    Ok(())
}

pub fn create_output_audio_task() -> (Sender<OutputAudioTaskCommand>, JoinHandle<()>) {
    let (command_tx, command_rx): (
        Sender<OutputAudioTaskCommand>,
        Receiver<OutputAudioTaskCommand>,
    ) = unbounded();

    let thread = spawn(move || {
        if let Err(e) = output_audio(command_rx) {
            eprintln!("Error in output_audio: {}", e);
        }
    });

    (command_tx, thread)
}
