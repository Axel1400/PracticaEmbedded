use std::thread::{spawn, JoinHandle};

use crossbeam::channel::{Sender, Receiver, unbounded};
use evdev::Device;

use crate::terminal_task::CallScreenCommand;

pub enum EventCommand {
    Exit,
}

pub fn create_event_task(
    main_task_queue: Sender<CallScreenCommand>,
) -> (Sender<EventCommand>, JoinHandle<()>) {
    let (command_tx, command_rx): (Sender<EventCommand>, Receiver<EventCommand>) = unbounded();

    let thread = spawn(move || {
        if let Err(e) = event_task(command_rx, main_task_queue) {
            eprintln!("Error in event_task: {}", e);
        }
    });

    (command_tx, thread)
}

fn event_task(
    command_receiver: Receiver<EventCommand>,
    main_task_queue: Sender<CallScreenCommand>,
) -> anyhow::Result<()> {
    // Read linux event0
    let mut device = Device::open("/dev/input/event0")?;

    loop {
        if let Ok(ev) = command_receiver.try_recv() {
            match ev {
                EventCommand::Exit => {
                    break;
                }
            }
        }

        let events = device.fetch_events()?;
        for event in events {
            match event.kind() {
                evdev::InputEventKind::Synchronization(_) => todo!(),
                evdev::InputEventKind::Key(_) => todo!(),
                evdev::InputEventKind::RelAxis(_) => todo!(),
                evdev::InputEventKind::AbsAxis(_) => todo!(),
                evdev::InputEventKind::Misc(_) => todo!(),
                evdev::InputEventKind::Switch(_) => todo!(),
                evdev::InputEventKind::Led(_) => todo!(),
                evdev::InputEventKind::Sound(_) => todo!(),
                evdev::InputEventKind::ForceFeedback(_) => todo!(),
                evdev::InputEventKind::ForceFeedbackStatus(_) => todo!(),
                evdev::InputEventKind::UInput(_) => todo!(),
                evdev::InputEventKind::Other => todo!(),
            }
        }
    }

    Ok(())
}
