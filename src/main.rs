mod input_audio_task;
mod network_thread;
mod output_audio_task;
mod terminal_task;

fn main() -> anyhow::Result<()> {
    let (output_audio_sender, output_audio_thread) = output_audio_task::create_output_audio_task();
    let (network_thread, network_sender) = network_thread::create_network_task()?;
    let (input_audio_thread, input_audio_sender) =
        input_audio_task::create_input_audio_task(network_sender.clone())?;

    let (terminal_thread, terminal_tx) =
        terminal_task::create_terminal_task(output_audio_sender.clone(), network_sender.clone());

    network_sender.send(network_thread::NetworkTaskCommand::MainTaskQueue(
        terminal_tx,
    ))?;

    let _ = terminal_thread.join();
    output_audio_sender.send(output_audio_task::OutputAudioTaskCommand::Exit)?;
    input_audio_sender.send(input_audio_task::InputAudioCommand::Exit)?;
    network_sender.send(network_thread::NetworkTaskCommand::Exit)?;
    let _ = output_audio_thread.join();
    let _ = input_audio_thread.join();
    let _ = network_thread.join();

    Ok(())
}
