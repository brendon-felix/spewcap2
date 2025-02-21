use std::sync::{Arc, Mutex};
use serialport5::{self, SerialPort, SerialPortBuilder};
use std::io::{self, BufWriter, Read, Write};
use crate::settings::Settings;
use crate::state::State;
use crate::utils::{did_quit, print_separator, sleep};

//REMOVE THIS LATER
use crate::state::ConnectionStatus;

struct Buffer {
    buffer: [u8; 1024],
    index: usize,
    line_index: usize,
}
impl Buffer {
    fn new() -> Self {
        Buffer {
            buffer: [0; 1024],
            index: 0,
            line_index: 0,
        }
    }
    fn write(&mut self, data_buffer: &[u8], data_size: usize) {
        let remaining_buffer_space = self.buffer.len() - self.index;
        let num_bytes = remaining_buffer_space.min(data_size); // only use the remaining space available
        self.buffer[self.index.. self.index + num_bytes].copy_from_slice(&data_buffer[..num_bytes]);
        self.index += num_bytes;
    }
    fn get_line(&mut self) -> Option<&str> {
        if let Some(newline_index) = self.buffer[self.line_index..self.index].iter().position(|&b| b == b'\n') {
            let line_end = self.line_index + newline_index + 1;
            let line_bytes = &self.buffer[self.line_index..line_end];
            self.line_index = line_end;
            let line = std::str::from_utf8(line_bytes).expect("Could not read line");
            Some(line)
        } else {
            None
        }
    }
    fn shift_remaining(&mut self) {
        let remaining_bytes = self.index - self.line_index;
        self.buffer.copy_within(self.line_index..self.index, 0);
        self.line_index = 0;
        self.index = remaining_bytes;
    }
}


pub fn connect_loop(settings: Settings, shared_state: Arc<Mutex<State>>) {
    // println!("SERIAL LOOP");
    let mut first_attempt = true;
    let mut status: ConnectionStatus;
    let port_name = &settings.port;
    loop {
        match open_serial_port(port_name, settings.baud_rate) {
            Some(port) => {
                status = ConnectionStatus::Connected;
                print_status_msg(port_name, status);
                // print_separator("");
                let mut stdout = Box::new(BufWriter::with_capacity(1024, io::stdout()));
                let quitting = read_loop(port, Arc::clone(&shared_state), &mut stdout);
                if quitting {
                    break;
                } else {
                    status = ConnectionStatus::Disconnected;
                    print_status_msg(port_name, status);
                }
            }
            None => {
                if first_attempt {
                    status = ConnectionStatus::NotConnected;
                    print_status_msg(port_name, status);
                }
                sleep(500);
            }
        }
        first_attempt = false;
    }
}

fn print_status_msg(port_name: &str, status: ConnectionStatus) {
    print_separator(format!("{} {}", port_name, status));
}

fn open_serial_port(port: &str, baud_rate: u32) -> Option<SerialPort> {
    let baud_rate = baud_rate;
    SerialPortBuilder::new()
        .baud_rate(baud_rate)
        .open(port).ok()
}

fn read_loop<W: Write>(mut port: SerialPort, shared_state: Arc<Mutex<State>>, stdout: &mut W) -> bool {
    let mut buffer = Buffer::new();
    loop {
        if did_quit(&shared_state) {
            return true;
        }
        let mut data_buffer = [0; 256];
        if port.bytes_to_read().unwrap_or(0) == 0 {
            sleep(100);
            continue;
        }
        match port.read(&mut data_buffer) {
            Ok(0) => return false,
            Ok(data_size) => {
                buffer.write(&data_buffer, data_size);
                while let Some(line) = buffer.get_line() {
                    output_line(line, stdout, &shared_state);
                }
                stdout.flush().expect("Failed to flush stdout");
                buffer.shift_remaining();
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => return false,
            Err(e) => {
                // println!("Failed to read port: {}", e);
                eprintln!("Failed to read port: {}", e);
                let mut state = shared_state.lock().unwrap();
                state.connection_status = ConnectionStatus::NotConnected;
                state.quitting = true;
                return false;
                // std::process::exit(0);
            }
        }
    }
}

fn output_line<W: Write>(line: &str, stdout: &mut W, shared_state: &Arc<Mutex<State>>) {
    let mut state = shared_state.lock().unwrap();
    stdout.write_all(line.as_bytes()).expect("Failed to write to stdout");
    if let Some(log) = &mut state.log {
        if log.enabled {
            log.write_line(line);
        }
    }
}