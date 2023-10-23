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

const INPUT_HARDWARE_NAME: &str = "hw:0";
// const INPUT_HARDWARE_NAME: &str = "default";

fn input_audio_task(
    command_receiver: Receiver<InputAudioCommand>,
    output_audio_sender: Sender<NetworkTaskCommand>,
) -> anyhow::Result<()> {
    let pcm = PCM::new(INPUT_HARDWARE_NAME, Direction::Capture, false)?;

    let hw_params = HwParams::any(&pcm)?;
    hw_params.set_access(alsa::pcm::Access::RWInterleaved)?;
    hw_params.set_format(alsa::pcm::Format::S32LE)?;
    hw_params.set_channels(2)?;
    hw_params.set_rate(48000, alsa::ValueOr::Nearest)?;
    hw_params.set_buffer_size_near(1024 * 1024)?;
    pcm.hw_params(&hw_params)?;
    pcm.start()?;

    let io = pcm.io_i32()?;
    let mut buffer = vec![0; 8192 * 2 * 2];

    let mut record = false;

    loop {
        if record {
            match io.readi(&mut buffer) {
                Ok(read) => {
                    if read > 0 {
                        // Convert i32 to i16, each i32 is 2 i16s
                        let mut send_buffer = Vec::with_capacity(read * 2);
                        for i in 0..read {
                            send_buffer.push((buffer[i * 2] / 256) as i16);
                            send_buffer.push((buffer[i * 2 + 1] / 256) as i16);
                        }

                        
                        output_audio_sender.send(NetworkTaskCommand::SendAudio(send_buffer))?;
                        buffer.clear();
                        println!("Sent audio");
                    } else {
                        // Start capture again
                        pcm.prepare()?;
                        pcm.start()?;
                    }
                }
                Err(e) => {
                    println!("Error reading audio: {}", e);
                }
            }
        }
        match command_receiver.recv_timeout(std::time::Duration::from_millis(5)) {
            Ok(InputAudioCommand::Start) => {
                record = true;
                pcm.prepare()?;
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
