use std::{
    thread::{spawn, JoinHandle},
    time::Duration,
};

use crossbeam::channel::{unbounded, Receiver, Sender};

use crate::{output_audio_task::OutputAudioTaskCommand, terminal_task::CallScreenCommand};

pub enum NetworkTaskCommand {
    StartConnection(std::net::SocketAddr),
    StopConnection,
    SendAccept,
    SendAudio(Vec<i16>),
    MainTaskQueue(Sender<CallScreenCommand>),
    OutputAudioQueue(Sender<OutputAudioTaskCommand>),
    Exit,
}

#[derive(PartialEq)]
enum NetworkState {
    PendingConnection(std::net::SocketAddr),
    InCall(std::net::SocketAddr),
    Stopped,
}

#[repr(u8)]
#[derive(PartialEq, Copy, Clone)]
enum NetworkPacketType {
    StartConnection = 0,
    StopConnection,
    Audio,
    Heartbeat,
    Accept,
}

#[derive(PartialEq, Clone)]
struct NetworkPacket {
    packet_type: NetworkPacketType,
    data: Vec<u8>,
}

impl NetworkPacket {
    fn new_start_connection() -> Self {
        Self {
            packet_type: NetworkPacketType::StartConnection,
            data: Vec::new(),
        }
    }

    fn new_stop_connection() -> Self {
        Self {
            packet_type: NetworkPacketType::StopConnection,
            data: Vec::new(),
        }
    }

    fn new_audio(audio: Vec<i16>) -> Self {
        // Use unsafe to convert the i32 slice to a u8 slice
        let data = unsafe {
            Vec::from_raw_parts(
                audio.as_ptr() as *mut u8,
                audio.len() * 2,
                audio.capacity() * 2,
            )
        };

        // Forget the audio so it doesn't get dropped
        std::mem::forget(audio);

        Self {
            packet_type: NetworkPacketType::Audio,
            data,
        }
    }

    fn new_accept() -> Self {
        Self {
            packet_type: NetworkPacketType::Accept,
            data: Vec::new(),
        }
    }

    fn new_heartbeat() -> Self {
        Self {
            packet_type: NetworkPacketType::Heartbeat,
            data: Vec::new(),
        }
    }

    fn serialize(&self) -> Vec<u8> {
        let mut buffer = Vec::new();
        buffer.push(self.packet_type as u8);
        buffer.extend_from_slice(&self.data);
        buffer
    }

    fn deserialize(data: Vec<u8>) -> Self {
        let packet_type = match data[0] {
            0 => NetworkPacketType::StartConnection,
            1 => NetworkPacketType::StopConnection,
            2 => NetworkPacketType::Audio,
            3 => NetworkPacketType::Heartbeat,
            4 => NetworkPacketType::Accept,
            _ => panic!("Invalid packet type"),
        };
        if data.len() == 1 {
            Self {
                packet_type,
                data: Vec::new(),
            }
        } else {
            let data = data[1..].to_vec();
            Self { packet_type, data }
        }
    }
}

fn network_task(rx: Receiver<NetworkTaskCommand>) -> anyhow::Result<()> {
    let mut current_state = NetworkState::Stopped;
    let udp_socket = std::net::UdpSocket::bind("0.0.0.0:33445")?;
    let main_thread_sender = {
        if let NetworkTaskCommand::MainTaskQueue(sender) = rx.recv()? {
            sender
        } else {
            panic!("Invalid command");
        }
    };
    let output_audio_sender = {
        if let NetworkTaskCommand::OutputAudioQueue(sender) = rx.recv()? {
            sender
        } else {
            panic!("Invalid command");
        }
    };
    // Set read timeout to 50ms
    udp_socket.set_read_timeout(Some(Duration::from_millis(1)))?;

    loop {
        match current_state {
            NetworkState::PendingConnection(peer) => {
                let mut buffer = [0; 1024];
                if let Ok(data) = udp_socket.recv_from(&mut buffer) {
                    if data.0 != 0 {
                        let packet = NetworkPacket::deserialize(buffer[..data.0].to_vec());
                        if packet.packet_type == NetworkPacketType::Accept {
                            // We are now in a call
                            current_state = NetworkState::InCall(peer);
                            main_thread_sender.send(CallScreenCommand::StartCall(peer))?;
                        } else if packet.packet_type == NetworkPacketType::StopConnection {
                            current_state = NetworkState::Stopped;
                            main_thread_sender.send(CallScreenCommand::StopCall)?;
                        }
                    }
                }
            }
            NetworkState::InCall(_) => {
                let mut buffer = vec![0; 1024 * 32 * 2];
                if let Ok(data) = udp_socket.recv_from(&mut buffer) {
                    if data.0 != 0 {
                        let packet = NetworkPacket::deserialize(buffer[..data.0].to_vec());
                        if packet.packet_type == NetworkPacketType::Audio {
                            // Send the audio to the main thread
                            let audio = unsafe {
                                Vec::from_raw_parts(
                                    packet.data.as_ptr() as *mut i16,
                                    packet.data.len() / 2,
                                    packet.data.capacity() / 2,
                                )
                            };
                            std::mem::forget(packet.data);
                            output_audio_sender.send(OutputAudioTaskCommand::Play(audio))?;
                        } else if packet.packet_type == NetworkPacketType::StopConnection {
                            current_state = NetworkState::Stopped;
                            main_thread_sender.send(CallScreenCommand::StopCall)?;
                        }
                    }
                }
            }
            NetworkState::Stopped => {
                // We are in a valid state to receive a call
                let mut buffer = [0; 1024];
                if let Ok((data, peer)) = udp_socket.recv_from(&mut buffer) {
                    if data != 0 {
                        let packet = NetworkPacket::deserialize(buffer[..data].to_vec());
                        if packet.packet_type == NetworkPacketType::StartConnection {
                            current_state = NetworkState::PendingConnection(peer);

                            // Send a heartbeat
                            let packet = NetworkPacket::new_heartbeat();
                            let packet = packet.serialize();
                            udp_socket.send_to(&packet, peer)?;

                            // Send a command to the main thread to start the call screen
                            main_thread_sender.send(CallScreenCommand::IncomingCall(peer))?;
                        }
                    }
                }
            }
        }

        //We need to check if we are in a valid state for receiving a call (base state, not in call, not made a call)
        if let NetworkState::Stopped = current_state {}
        match rx.try_recv() {
            Ok(NetworkTaskCommand::StartConnection(message)) => {
                // Start the network connection
                let packet = NetworkPacket::new_start_connection();
                let serialized_packet = packet.serialize();
                udp_socket.send_to(&serialized_packet, message)?;

                // Wait for a response, it should be a heartbeat
                current_state = NetworkState::PendingConnection(message);
            }
            Ok(NetworkTaskCommand::SendAccept) => {
                if let NetworkState::PendingConnection(peer) = current_state {
                    // Send an accept packet
                    let packet = NetworkPacket::new_accept();
                    let serialized_packet = packet.serialize();
                    udp_socket.send_to(&serialized_packet, peer)?;
                    current_state = NetworkState::InCall(peer);
                }
            }
            Ok(NetworkTaskCommand::StopConnection) => {
                if let NetworkState::InCall(peer) = current_state {
                    // Send a stop connection packet
                    let packet = NetworkPacket::new_stop_connection();
                    let serialized_packet = packet.serialize();
                    udp_socket.send_to(&serialized_packet, peer)?;
                }
                current_state = NetworkState::Stopped;
            }
            Ok(NetworkTaskCommand::SendAudio(audio)) => {
                if let NetworkState::InCall(remote_peer) = current_state {
                    // Send audio
                    let packet = NetworkPacket::new_audio(audio);
                    let serialized_packet = packet.serialize();
                    udp_socket.send_to(&serialized_packet, remote_peer)?;
                }
            }
            Ok(NetworkTaskCommand::Exit) => {
                break;
            }
            Ok(NetworkTaskCommand::MainTaskQueue(_)) => {}
            Ok(NetworkTaskCommand::OutputAudioQueue(_)) => {}
            Err(_) => {
                // Do nothing
            }
        }
    }
    Ok(())
}

pub fn create_network_task() -> anyhow::Result<(JoinHandle<()>, Sender<NetworkTaskCommand>)> {
    let (sender, receiver) = unbounded::<NetworkTaskCommand>();

    let join = spawn(move || {
        if let Err(e) = network_task(receiver) {
            eprintln!("Error in network_task: {}", e);
        }
    });

    Ok((join, sender))
}
