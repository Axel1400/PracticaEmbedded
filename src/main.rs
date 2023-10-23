use std::io::stdout;

use crossterm::{
    terminal::{disable_raw_mode, LeaveAlternateScreen},
    ExecutableCommand,
};

mod events;
mod input_audio_task;
mod network_thread;
mod output_audio_task;
mod terminal_task;
mod utils;

const READY_TO_PAIR_SOUND: &[u8] = include_bytes!("assets/ready_to_pair.mp3");

fn main() -> anyhow::Result<()> {
    std::panic::set_hook(Box::new(|panic_info| {
        eprintln!("Panic: {}", panic_info);
        let _ = disable_raw_mode();
        let _ = stdout().execute(LeaveAlternateScreen);
        std::process::exit(1);
    }));
    let (output_audio_sender, output_audio_thread) = output_audio_task::create_output_audio_task();
    let (network_thread, network_sender) = network_thread::create_network_task()?;
    let (input_audio_thread, input_audio_sender) =
        input_audio_task::create_input_audio_task(network_sender.clone())?;

    let (terminal_thread, terminal_tx) = terminal_task::create_terminal_task(
        output_audio_sender.clone(),
        input_audio_sender.clone(),
        network_sender.clone(),
    );

    std::thread::sleep(std::time::Duration::from_millis(100));
    network_sender.send(network_thread::NetworkTaskCommand::MainTaskQueue(
        terminal_tx,
    ))?;
    network_sender.send(network_thread::NetworkTaskCommand::OutputAudioQueue(
        output_audio_sender.clone(),
    ))?;

    let play_buffer = utils::decode_bytes(READY_TO_PAIR_SOUND);

    output_audio_sender.send(output_audio_task::OutputAudioTaskCommand::Play(play_buffer))?;

    let _ = terminal_thread.join();
    output_audio_sender.send(output_audio_task::OutputAudioTaskCommand::Exit)?;
    input_audio_sender.send(input_audio_task::InputAudioCommand::Exit)?;
    network_sender.send(network_thread::NetworkTaskCommand::Exit)?;
    let _ = output_audio_thread.join();
    let _ = input_audio_thread.join();
    let _ = network_thread.join();

    Ok(())
}
