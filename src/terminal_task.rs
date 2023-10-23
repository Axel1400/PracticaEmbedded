use crate::{
    input_audio_task::InputAudioCommand, network_thread::NetworkTaskCommand,
    output_audio_task::OutputAudioTaskCommand,
};
use crossbeam::channel::{unbounded, Receiver, Sender};
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{prelude::*, widgets::*};
use std::{
    fs::File,
    io::{stdout, Write},
    net::{IpAddr, SocketAddr},
    thread::{self, JoinHandle},
};

struct StatefulList<T> {
    state: ListState,
    items: Vec<T>,
}

impl<T> StatefulList<T> {
    fn with_items(items: Vec<T>) -> StatefulList<T> {
        StatefulList {
            state: ListState::default(),
            items,
        }
    }

    fn next(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i >= self.items.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    fn previous(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i == 0 {
                    self.items.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }
}

struct HomeScreenState {
    pub menu_list_state: StatefulList<&'static str>,
}

impl HomeScreenState {
    pub fn new() -> HomeScreenState {
        let mut menu_list_state = StatefulList::with_items(vec!["Call", "Contacts", "Exit"]);
        menu_list_state.next();
        HomeScreenState {
            menu_list_state: menu_list_state,
        }
    }
}

#[derive(Clone, Copy)]
enum CallScreenStatus {
    Calling,
    IncomingCall,
    InCall {
        start_time: std::time::Instant,
    },
    CallEnded {
        start_time: std::time::Instant,
        end_time: std::time::Instant,
    },
}

pub enum CallScreenCommand {
    StartCall(SocketAddr),
    IncomingCall(SocketAddr),
    StopCall,
    EndCall,
    AcceptCall,
    RejectCall,
    IncreaseVolume,
    DecreaseVolume,
    ToggleMute,
    Exit,
}

struct CallScreenState {
    pub is_muted: bool,
    pub volume: i64,
    pub remote_ip: std::net::IpAddr,
    pub remote_name: Option<String>,
    pub call_status: CallScreenStatus,
}

impl CallScreenState {
    pub fn new(ip: IpAddr) -> CallScreenState {
        CallScreenState {
            is_muted: false,
            volume: 100,
            call_status: CallScreenStatus::Calling,
            remote_ip: ip,
            remote_name: None,
        }
    }
}

pub struct ContactsScreenState {
    pub contacts: Vec<String>,
    pub selected_contact: Option<usize>,
    pub new_contact_name: String,
    pub new_contact_ip: String,
}

impl ContactsScreenState {
    pub fn new() -> ContactsScreenState {
        ContactsScreenState {
            contacts: Vec::new(),
            selected_contact: None,
            new_contact_name: String::new(),
            new_contact_ip: String::new(),
        }
    }
}

struct CallInfoScreenState {
    pub ip: String,
}

enum ScreenState {
    Home(HomeScreenState),
    Contacts(ContactsScreenState),
    EnterCallInfo(CallInfoScreenState),
    Call(CallScreenState),
}

struct LedInfo {
    red: File,
    green: File,
    blue: File,
}

struct AppState {
    pub output_audio_sender: Sender<OutputAudioTaskCommand>,
    pub input_audio_sender: Sender<InputAudioCommand>,
    pub network_sender: Sender<NetworkTaskCommand>,
    pub call_rx: Receiver<CallScreenCommand>,
    pub screen_state: ScreenState,
    pub led_0: LedInfo,
    pub led_1: LedInfo,
    pub led_2: LedInfo,
    pub animation_state: u8,
}

impl AppState {
    pub fn new(
        output_audio_sender: Sender<OutputAudioTaskCommand>,
        input_audio_sender: Sender<InputAudioCommand>,
        network_sender: Sender<NetworkTaskCommand>,
        call_rx: Receiver<CallScreenCommand>,
    ) -> AppState {
        let led_0 = LedInfo {
            blue: std::fs::OpenOptions::new()
                .write(true)
                .open("/sys/class/leds/pca995x:blue0/brightness")
                .unwrap(),
            green: std::fs::OpenOptions::new()
                .write(true)
                .open("/sys/class/leds/pca995x:green0/brightness")
                .unwrap(),
            red: std::fs::OpenOptions::new()
                .write(true)
                .open("/sys/class/leds/pca995x:red0/brightness")
                .unwrap(),
        };
        let led_1 = LedInfo {
            blue: std::fs::OpenOptions::new()
                .write(true)
                .open("/sys/class/leds/pca995x:blue1/brightness")
                .unwrap(),
            green: std::fs::OpenOptions::new()
                .write(true)
                .open("/sys/class/leds/pca995x:green1/brightness")
                .unwrap(),
            red: std::fs::OpenOptions::new()
                .write(true)
                .open("/sys/class/leds/pca995x:red1/brightness")
                .unwrap(),
        };
        let led_2 = LedInfo {
            blue: std::fs::OpenOptions::new()
                .write(true)
                .open("/sys/class/leds/pca995x:blue2/brightness")
                .unwrap(),
            green: std::fs::OpenOptions::new()
                .write(true)
                .open("/sys/class/leds/pca995x:green2/brightness")
                .unwrap(),
            red: std::fs::OpenOptions::new()
                .write(true)
                .open("/sys/class/leds/pca995x:red2/brightness")
                .unwrap(),
        };

        AppState {
            output_audio_sender,
            input_audio_sender,
            network_sender,
            call_rx,
            led_0,
            led_1,
            led_2,
            animation_state: 0,
            screen_state: ScreenState::Home(HomeScreenState::new()),
        }
    }

    fn animation(&mut self) {
        // Animation is:
        // 0: All off
        // 1: All on
        // 2: led0.red on, led1.green on, led2.blue on
        // 3: led0.green on, led1.blue on, led2.red on
        // 4: led0.blue on, led1.red on, led2.green on

        let led0 = &mut self.led_0;
        let led1 = &mut self.led_1;
        let led2 = &mut self.led_2;

        match self.animation_state {
            0 => {
                led0.red.write_all(b"0").unwrap();
                led0.green.write_all(b"0").unwrap();
                led0.blue.write_all(b"0").unwrap();

                led1.red.write_all(b"0").unwrap();
                led1.green.write_all(b"0").unwrap();
                led1.blue.write_all(b"0").unwrap();

                led2.red.write_all(b"0").unwrap();
                led2.green.write_all(b"0").unwrap();
                led2.blue.write_all(b"0").unwrap();

                self.animation_state = 1;
            }
            1 => {
                led0.red.write_all(b"128").unwrap();
                led0.green.write_all(b"128").unwrap();
                led0.blue.write_all(b"128").unwrap();

                led1.red.write_all(b"128").unwrap();
                led1.green.write_all(b"128").unwrap();
                led1.blue.write_all(b"128").unwrap();

                led2.red.write_all(b"128").unwrap();
                led2.green.write_all(b"128").unwrap();
                led2.blue.write_all(b"128").unwrap();

                self.animation_state = 2;
            }
            2 => {
                led0.red.write_all(b"128").unwrap();
                led0.green.write_all(b"0").unwrap();
                led0.blue.write_all(b"0").unwrap();

                led1.red.write_all(b"0").unwrap();
                led1.green.write_all(b"128").unwrap();
                led1.blue.write_all(b"0").unwrap();

                led2.red.write_all(b"0").unwrap();
                led2.green.write_all(b"0").unwrap();
                led2.blue.write_all(b"128").unwrap();

                self.animation_state = 3;
            }
            3 => {
                led0.red.write_all(b"0").unwrap();
                led0.green.write_all(b"128").unwrap();
                led0.blue.write_all(b"0").unwrap();

                led1.red.write_all(b"0").unwrap();
                led1.green.write_all(b"0").unwrap();
                led1.blue.write_all(b"128").unwrap();

                led2.red.write_all(b"128").unwrap();
                led2.green.write_all(b"0").unwrap();
                led2.blue.write_all(b"0").unwrap();

                self.animation_state = 4;
            }
            4 => {
                led0.red.write_all(b"0").unwrap();
                led0.green.write_all(b"0").unwrap();
                led0.blue.write_all(b"128").unwrap();

                led1.red.write_all(b"128").unwrap();
                led1.green.write_all(b"0").unwrap();
                led1.blue.write_all(b"0").unwrap();

                led2.red.write_all(b"0").unwrap();
                led2.green.write_all(b"128").unwrap();
                led2.blue.write_all(b"0").unwrap();

                self.animation_state = 0;
            }
            _ => unreachable!(),
        }
    }

    fn stop_animation(&mut self) {
        self.animation_state = 0;
        let led0 = &mut self.led_0;
        let led1 = &mut self.led_1;
        let led2 = &mut self.led_2;

        led0.red.write_all(b"0").unwrap();
        led0.green.write_all(b"0").unwrap();
        led0.blue.write_all(b"0").unwrap();

        led1.red.write_all(b"0").unwrap();
        led1.green.write_all(b"0").unwrap();
        led1.blue.write_all(b"0").unwrap();

        led2.red.write_all(b"0").unwrap();
        led2.green.write_all(b"0").unwrap();
        led2.blue.write_all(b"0").unwrap();
    }
}

fn main_screen<B: Backend>(f: &mut Frame<B>, state: &mut HomeScreenState) {
    let menu_items = state
        .menu_list_state
        .items
        .iter()
        .map(|i| ListItem::new(Span::raw(*i)))
        .collect::<Vec<ListItem>>();

    let menu = List::new(menu_items)
        .block(Block::default().title("Menu").borders(Borders::ALL))
        .highlight_style(Style::default().add_modifier(Modifier::BOLD))
        .highlight_symbol(">> ");

    // Draw the menu
    f.render_stateful_widget(menu, f.size(), &mut state.menu_list_state.state);
}

fn enter_call_info<B: Backend>(f: &mut Frame<B>, state: &mut CallInfoScreenState) {
    // Render an input box for the IP in the center of the screen
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(30),
            Constraint::Percentage(40),
            Constraint::Percentage(30),
        ])
        .split(f.size());

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[1]);

    let ip_input = Paragraph::new(format!("IP: {}", state.ip)).alignment(Alignment::Center);

    let press_enter =
        Paragraph::new("Press enter to continue or esc to cancel").alignment(Alignment::Center);

    f.render_widget(ip_input, layout[0]);
    f.render_widget(press_enter, layout[1]);
}

fn call_screen_controls<B: Backend>(f: &mut Frame<B>, rect: Rect, state: &mut CallScreenState) {
    // Control to mute the call, show the volume, and end the call
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(10),
            Constraint::Percentage(80),
            Constraint::Percentage(10),
        ])
        .split(rect);

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(33),
            Constraint::Percentage(33),
            Constraint::Percentage(33),
        ])
        .split(chunks[1]);

    let mute_button = Span::styled(
        if state.is_muted { "Unmute" } else { "Mute" },
        Style::default()
            .add_modifier(Modifier::BOLD)
            .add_modifier(if state.is_muted {
                Modifier::CROSSED_OUT
            } else {
                Modifier::empty()
            }),
    );
    let mute_paragraph = Paragraph::new(mute_button).alignment(Alignment::Center);

    let volume = Paragraph::new(format!("Volume: {}", state.volume)).alignment(Alignment::Center);

    let end_call = Paragraph::new("End Call").alignment(Alignment::Center);

    f.render_widget(mute_paragraph, chunks[0]);
    f.render_widget(volume, chunks[1]);
    f.render_widget(end_call, chunks[2]);
}

fn call_screen<B: Backend>(f: &mut Frame<B>, state: &mut CallScreenState) {
    // Split screen in 2, top half is for the call info, bottom half is for the controls

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(80), Constraint::Percentage(20)])
        .split(f.size());

    // Draw the call info

    // Split into 4 chunks, start and end to fill the screen, and the middle 2 for the call info
    let call_info_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(25),
            Constraint::Percentage(50),
            Constraint::Percentage(25),
        ])
        .split(chunks[0]);
    let call_info_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(40),
            Constraint::Percentage(20),
            Constraint::Percentage(40),
        ])
        .split(call_info_chunks[1]);

    let name = (state.remote_name.clone()).unwrap_or(state.remote_ip.to_string());
    let elapsed_time = match state.call_status {
        CallScreenStatus::Calling => {
            format!("Calling {}...", name)
        }
        CallScreenStatus::IncomingCall => {
            format!("Incoming call from {}...", name)
        }
        CallScreenStatus::InCall { start_time } => {
            let elapsed = start_time.elapsed();
            let minutes = elapsed.as_secs() / 60;
            let seconds = elapsed.as_secs() % 60;
            format!("In call with {} for {}:{:02}", name, minutes, seconds)
        }
        CallScreenStatus::CallEnded {
            start_time,
            end_time,
        } => {
            let elapsed = end_time.duration_since(start_time);
            format!(
                "Call with {} ended after {}:{:02}",
                name,
                elapsed.as_secs(),
                elapsed.subsec_millis() / 10
            )
        }
    };

    let call_info_block = Block::default().title("Call Info").borders(Borders::ALL);
    let call_info = Paragraph::new(elapsed_time).alignment(Alignment::Center);

    f.render_widget(call_info_block, chunks[0]);
    // Render the elapsed time
    f.render_widget(call_info, call_info_chunks[1]);
    // Draw the controls
    call_screen_controls(f, chunks[1], state);
}

fn ui<B: Backend>(f: &mut Frame<B>, app: &mut AppState) {
    match &mut app.screen_state {
        ScreenState::Home(home_state) => {
            main_screen(f, home_state);
        }
        ScreenState::EnterCallInfo(state) => {
            enter_call_info(f, state);
        }
        ScreenState::Contacts(_) => todo!(),
        ScreenState::Call(call_state) => call_screen(f, call_state),
    }
}

fn run_terminal_task(
    output_audio: Sender<OutputAudioTaskCommand>,
    input_audio: Sender<InputAudioCommand>,
    network_queue: Sender<NetworkTaskCommand>,
    call_rx: Receiver<CallScreenCommand>,
) -> anyhow::Result<()> {
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;

    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

    let mut should_quit = false;

    let mut app = AppState::new(output_audio, input_audio, network_queue, call_rx);

    while !should_quit {
        terminal.draw(|f| {
            ui(f, &mut app);
        })?;

        should_quit = handle_events(&mut app)?;
    }

    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;

    Ok(())
}

pub fn create_terminal_task(
    output_audio: Sender<OutputAudioTaskCommand>,
    input_audio: Sender<InputAudioCommand>,
    network_queue: Sender<NetworkTaskCommand>,
) -> (JoinHandle<()>, Sender<CallScreenCommand>) {
    let (call_tx, call_rx) = unbounded();
    let thread = thread::spawn(move || {
        if let Err(e) = run_terminal_task(output_audio, input_audio, network_queue, call_rx) {
            eprintln!("Error: {}", e);
        }
    });

    (thread, call_tx)
}

fn handle_events(app: &mut AppState) -> anyhow::Result<bool> {
    // Check if we have a call request
    if let Ok(cmd) = app.call_rx.try_recv() {
        match cmd {
            CallScreenCommand::StartCall(sock) => {
                let call_screen_state = {
                    let mut call_screen_state = CallScreenState::new(sock.ip());
                    call_screen_state.call_status = CallScreenStatus::InCall {
                        start_time: std::time::Instant::now(),
                    };
                    app.output_audio_sender.send(OutputAudioTaskCommand::Stop)?;
                    app.input_audio_sender.send(InputAudioCommand::Start)?;
                    call_screen_state
                };
                app.screen_state = ScreenState::Call(call_screen_state);
            }
            CallScreenCommand::IncomingCall(sock) => {
                let call_screen_state = {
                    let mut call_screen_state = CallScreenState::new(sock.ip());
                    call_screen_state.call_status = CallScreenStatus::IncomingCall;
                    app.output_audio_sender.send(OutputAudioTaskCommand::Stop)?;
                    call_screen_state
                };
                const INCOMING_CALL_SOUND: &[u8] = include_bytes!("assets/capitao_whatsapp.mp3");
                let play_buffer = crate::utils::decode_bytes(INCOMING_CALL_SOUND);
                app.output_audio_sender
                    .send(OutputAudioTaskCommand::Play(play_buffer))?;
                app.screen_state = ScreenState::Call(call_screen_state);
            }
            CallScreenCommand::StopCall => {
                if let ScreenState::Call(_) = &mut app.screen_state {
                    app.screen_state = ScreenState::Home(HomeScreenState::new());
                    app.network_sender
                        .send(NetworkTaskCommand::StopConnection)?;
                    app.input_audio_sender.send(InputAudioCommand::Stop)?;
                    app.output_audio_sender.send(OutputAudioTaskCommand::Stop)?;
                    app.stop_animation();
                }
            }
            CallScreenCommand::AcceptCall => {
                // Accept the call
                if let ScreenState::Call(call_state) = &mut app.screen_state {
                    app.network_sender.send(NetworkTaskCommand::SendAccept)?;
                    app.input_audio_sender.send(InputAudioCommand::Start)?;
                    call_state.call_status = CallScreenStatus::InCall {
                        start_time: std::time::Instant::now(),
                    };
                    app.output_audio_sender.send(OutputAudioTaskCommand::Stop)?;
                    app.stop_animation();
                }
            }
            CallScreenCommand::RejectCall => {
                // Reject the call
                app.screen_state = ScreenState::Home(HomeScreenState::new());
                app.network_sender
                    .send(NetworkTaskCommand::StopConnection)?;
                app.input_audio_sender.send(InputAudioCommand::Stop)?;
                app.output_audio_sender.send(OutputAudioTaskCommand::Stop)?;
                app.stop_animation();
            }
            CallScreenCommand::EndCall => {
                // End the call
                app.network_sender
                    .send(NetworkTaskCommand::StopConnection)?;
                app.screen_state = ScreenState::Home(HomeScreenState::new());
            }
            CallScreenCommand::Exit => {
                return Ok(true);
            }
            CallScreenCommand::IncreaseVolume => {
                if let ScreenState::Call(call_state) = &mut app.screen_state {
                    call_state.volume = (call_state.volume + 5).min(100);
                    app.output_audio_sender
                        .send(OutputAudioTaskCommand::SetVolume(call_state.volume))?;
                }
            }
            CallScreenCommand::DecreaseVolume => {
                if let ScreenState::Call(call_state) = &mut app.screen_state {
                    call_state.volume = (call_state.volume - 5).max(0);
                    app.output_audio_sender
                        .send(OutputAudioTaskCommand::SetVolume(call_state.volume))?;
                }
            }
            CallScreenCommand::ToggleMute => {
                if let ScreenState::Call(call_state) = &mut app.screen_state {
                    call_state.is_muted = !call_state.is_muted;
                    app.output_audio_sender
                        .send(OutputAudioTaskCommand::SetMute(call_state.is_muted))?;
                }
            }
        }
    }

    if let ScreenState::Call(call_state) = &mut app.screen_state {
        if let CallScreenStatus::IncomingCall = call_state.call_status {
            app.animation();
        }
    }

    match &mut app.screen_state {
        ScreenState::Home(home_state) => {
            if !event::poll(std::time::Duration::from_millis(100))? {
                return Ok(false);
            }
            if let Event::Key(KeyEvent { code, .. }) = event::read()? {
                match code {
                    KeyCode::Up => {
                        home_state.menu_list_state.previous();
                    }
                    KeyCode::Down => {
                        home_state.menu_list_state.next();
                    }
                    KeyCode::Enter => match home_state.menu_list_state.state.selected() {
                        Some(0) => {
                            app.screen_state = ScreenState::EnterCallInfo(CallInfoScreenState {
                                ip: String::new(),
                            });
                        }
                        Some(1) => {
                            app.screen_state = ScreenState::Contacts(ContactsScreenState::new());
                        }
                        Some(2) => {
                            return Ok(true);
                        }
                        _ => {}
                    },
                    _ => {}
                }
            }
        }
        ScreenState::EnterCallInfo(state) => {
            if !event::poll(std::time::Duration::from_millis(100))? {
                return Ok(false);
            }
            if let Event::Key(KeyEvent { code, .. }) = event::read()? {
                match code {
                    KeyCode::Enter => {
                        if let Ok(ip) = state.ip.parse::<IpAddr>() {
                            app.screen_state = ScreenState::Call(CallScreenState::new(ip));
                            //After we checked the IP, we can start the call
                            //Create the socket IP address
                            let socket_addr = SocketAddr::new(ip, 33445);
                            app.network_sender
                                .send(NetworkTaskCommand::StartConnection(socket_addr))?;
                        }
                    }
                    KeyCode::Esc => {
                        app.screen_state = ScreenState::Home(HomeScreenState::new());
                    }
                    KeyCode::Char(c) => {
                        state.ip.push(c);
                    }
                    KeyCode::Backspace => {
                        state.ip.pop();
                    }
                    _ => {}
                }
            }
        }
        ScreenState::Contacts(_) => todo!(),
        ScreenState::Call(state) => {
            if !event::poll(std::time::Duration::from_millis(100))? {
                return Ok(false);
            }
            if let Event::Key(KeyEvent { code, .. }) = event::read()? {
                match code {
                    KeyCode::Esc => {
                        // TODO: End call should wait a few seconds before going back to the home screen
                        app.screen_state = ScreenState::Home(HomeScreenState::new());
                        app.network_sender
                            .send(NetworkTaskCommand::StopConnection)?;
                        app.input_audio_sender.send(InputAudioCommand::Stop)?;
                        app.output_audio_sender.send(OutputAudioTaskCommand::Stop)?;
                        app.stop_animation();
                    }
                    KeyCode::Char('m') => {
                        state.is_muted = !state.is_muted;
                        app.output_audio_sender
                            .send(OutputAudioTaskCommand::SetMute(state.is_muted))?;
                    }
                    KeyCode::Char('a') => {
                        state.volume = (state.volume - 5).max(0);
                        app.output_audio_sender
                            .send(OutputAudioTaskCommand::SetVolume(state.volume))?;
                    }
                    KeyCode::Char('d') => {
                        state.volume = (state.volume + 5).min(100);
                        app.output_audio_sender
                            .send(OutputAudioTaskCommand::SetVolume(state.volume))?;
                    }
                    KeyCode::Enter => {
                        if let CallScreenStatus::IncomingCall = state.call_status {
                            app.network_sender.send(NetworkTaskCommand::SendAccept)?;
                            app.input_audio_sender.send(InputAudioCommand::Start)?;
                            state.call_status = CallScreenStatus::InCall {
                                start_time: std::time::Instant::now(),
                            };
                            app.output_audio_sender.send(OutputAudioTaskCommand::Stop)?;
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    Ok(false)
}
