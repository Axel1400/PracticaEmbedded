use std::thread::{spawn, JoinHandle};

use crossbeam::channel::{unbounded, Receiver, Sender};
use evdev::{Device, InputEventKind, Key};

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
            if let InputEventKind::Key(k) = event.kind() {
                match k {
                    Key::KEY_UP => {
                        main_task_queue.send(CallScreenCommand::IncreaseVolume)?;
                    }
                    Key::KEY_DOWN => {
                        main_task_queue.send(CallScreenCommand::DecreaseVolume)?;
                    }
                    Key::KEY_MUTE => {
                        main_task_queue.send(CallScreenCommand::ToggleMute)?;
                    }
                    Key::KEY_SELECT => {
                        main_task_queue.send(CallScreenCommand::AcceptCall)?;
                    }
                    Key::KEY_OK => {
                        main_task_queue.send(CallScreenCommand::StopCall)?;
                    }
                    _ => {}
                }
            }
        }
    }

    Ok(())
}
