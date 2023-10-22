use std::thread::JoinHandle;

use alsa::{
    pcm::{HwParams, PCM},
    Direction,
};
use crossbeam::channel::{unbounded, Receiver, Sender};

use crate::network_thread::NetworkTaskCommand;

pub enum InputAudioCommand {
    Start,
    Stop,
    Exit,
}

const INPUT_HARDWARE_NAME: &str = "default";

fn input_audio_task(
    command_receiver: Receiver<InputAudioCommand>,
    output_audio_sender: Sender<NetworkTaskCommand>,
) -> anyhow::Result<()> {
    let pcm = PCM::new(INPUT_HARDWARE_NAME, Direction::Capture, true)?;

    let hw_params = HwParams::any(&pcm)?;
    hw_params.set_access(alsa::pcm::Access::RWInterleaved)?;
    hw_params.set_format(alsa::pcm::Format::S16LE)?;
    hw_params.set_channels(2)?;
    hw_params.set_rate(48000, alsa::ValueOr::Nearest)?;

    let io = pcm.io_i16()?;
    let mut buffer = vec![0; 8192];

    let mut record = false;

    loop {
        if record {
            let read = io.readi(&mut buffer)?;
            output_audio_sender.send(NetworkTaskCommand::SendAudio(buffer.clone()))?;
            buffer.clear();
        }
        match command_receiver.recv_timeout(std::time::Duration::from_millis(5)) {
            Ok(InputAudioCommand::Start) => {
                record = true;
            }
            Ok(InputAudioCommand::Stop) => {
                // Stop
                record = false;
                pcm.drop()?;
            }
            Ok(InputAudioCommand::Exit) => {
                break;
            }
            Err(_) => {
                // Do nothing
            }
        }
    }

    Ok(())
}

pub fn create_input_audio_task(
    network_sender: Sender<NetworkTaskCommand>,
) -> anyhow::Result<(JoinHandle<()>, Sender<InputAudioCommand>)> {
    let (command_sender, command_receiver) = unbounded::<InputAudioCommand>();

    let handle = std::thread::spawn(move || {
        if let Err(e) = input_audio_task(command_receiver, network_sender) {
            eprintln!("Error in input audio task: {}", e);
        }
    });

    Ok((handle, command_sender))
}
